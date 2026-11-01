//! Backend-only temporary credentials. No remote error body is surfaced or logged.
use crate::execution::{Bank, BankFailure, Destination, Operation};
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use zeroize::Zeroizing;

mod matching;
#[cfg(test)]
mod tests;

pub struct Monzo {
    client: Client,
    token: Zeroizing<String>,
    base_url: String,
    poll_delay: Duration,
}

struct RequestFailure {
    failure: BankFailure,
    status: Option<u16>,
    detail: &'static str,
}

fn preflight_failure(allocation: &str, check: &str, detail: impl AsRef<str>) -> BankFailure {
    let label = match allocation {
        "personal" => "Personal",
        "joint" => "Joint",
        _ => "Configured account",
    };
    let message = format!(
        "{label} preflight failed ({check}): {} No deposits were submitted by this preflight check.",
        detail.as_ref()
    );
    // Only locally composed diagnostics: no token, IDs, response bodies or request headers.
    eprintln!("{message}");
    BankFailure::Preflight(message)
}

impl Monzo {
    pub fn new(token: String) -> crate::Result<Self> {
        let token = Zeroizing::new(token);
        if token.trim().is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
            return Err(crate::Error::Invalid(
                "Provide a temporary Monzo access token.".into(),
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .build()
            .map_err(|_| crate::Error::Worker)?;
        Ok(Self {
            client,
            token,
            base_url: "https://api.monzo.com".into(),
            poll_delay: Duration::from_millis(250),
        })
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        fields: &[(&str, String)],
    ) -> std::result::Result<Value, BankFailure> {
        self.request_detailed(method, path, fields)
            .await
            .map_err(|e| e.failure)
    }

    async fn request_detailed(
        &self,
        method: Method,
        path: &str,
        fields: &[(&str, String)],
    ) -> std::result::Result<Value, RequestFailure> {
        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.base_url))
            .bearer_auth(self.token.as_str());
        request = if method == Method::GET {
            request.query(fields)
        } else {
            request.form(fields)
        };
        let response = request.send().await.map_err(|e| RequestFailure {
            failure: BankFailure::Uncertain,
            status: None,
            detail: if e.is_timeout() {
                "Monzo request timed out."
            } else {
                "Could not connect to Monzo or receive its response."
            },
        })?;
        let status = response.status();
        let failure = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Some((
                BankFailure::Unauthorized,
                "Check the token and approve API access in the Monzo app.",
            )),
            StatusCode::TOO_MANY_REQUESTS => Some((
                BankFailure::RateLimited,
                "Monzo rate limit reached; wait before retrying.",
            )),
            StatusCode::REQUEST_TIMEOUT | StatusCode::CONFLICT => Some((
                BankFailure::Uncertain,
                "Monzo could not complete the request; retry later.",
            )),
            s if s.is_client_error() => {
                Some((BankFailure::Rejected, "Monzo rejected this API request."))
            }
            s if !s.is_success() => Some((
                BankFailure::Uncertain,
                "Monzo returned an unsuccessful response; retry later.",
            )),
            _ => None,
        };
        if let Some((failure, detail)) = failure {
            return Err(RequestFailure {
                failure,
                status: Some(status.as_u16()),
                detail,
            });
        }
        response.json().await.map_err(|_| RequestFailure {
            failure: BankFailure::Uncertain,
            status: Some(status.as_u16()),
            detail: "Monzo returned an unreadable JSON response.",
        })
    }

    async fn preflight_request(
        &self,
        allocation: &str,
        path: &str,
        fields: &[(&str, String)],
    ) -> std::result::Result<Value, BankFailure> {
        self.request_detailed(Method::GET, path, fields)
            .await
            .map_err(|e| {
                let detail = if let Some(status) = e.status {
                    format!("HTTP {status}. {}", e.detail)
                } else {
                    e.detail.into()
                };
                preflight_failure(allocation, path, detail)
            })
    }

    async fn discover_transactions(
        &self,
        operations: &[Operation],
        used: &BTreeSet<String>,
    ) -> std::result::Result<Vec<(i64, String)>, BankFailure> {
        let mut accounts: BTreeMap<&str, Vec<&Operation>> = BTreeMap::new();
        for op in operations {
            accounts
                .entry(&op.destination.account_id)
                .or_default()
                .push(op);
        }
        let mut matches = Vec::new();
        for (account, ops) in accounts {
            let start = ops
                .iter()
                .map(|o| matching::timestamp(&o.started_at))
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .min()
                .ok_or(BankFailure::NoteMatchPending)?
                - chrono::Duration::seconds(5);
            let end = ops
                .iter()
                .map(|o| matching::timestamp(&o.confirmed_at))
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .max()
                .ok_or(BankFailure::NoteMatchPending)?
                + chrono::Duration::seconds(5);
            let mut since = start.to_rfc3339();
            let mut cursors = BTreeSet::new();
            let mut transactions = BTreeMap::new();
            let mut complete = false;
            for _ in 0..1000 {
                let page = self
                    .request(
                        Method::GET,
                        "/transactions",
                        &[
                            ("account_id", account.into()),
                            ("since", since.clone()),
                            ("before", end.to_rfc3339()),
                            ("limit", "100".into()),
                        ],
                    )
                    .await?;
                let rows = page["transactions"]
                    .as_array()
                    .ok_or(BankFailure::Uncertain)?;
                for tx in rows {
                    if let Some(id) = tx["id"].as_str() {
                        transactions.insert(id.to_owned(), tx.clone());
                    }
                }
                if rows.len() < 100 {
                    complete = true;
                    break;
                }
                let next = rows
                    .last()
                    .and_then(|r| r["id"].as_str())
                    .ok_or(BankFailure::Uncertain)?;
                if !cursors.insert(next.to_owned()) {
                    return Err(BankFailure::Uncertain);
                }
                since = next.into();
            }
            if !complete {
                return Err(BankFailure::Uncertain);
            }
            matches.extend(matching::match_transactions(
                &ops,
                &transactions.into_values().collect::<Vec<_>>(),
                used,
            )?);
        }
        Ok(matches)
    }
}
impl Bank for Monzo {
    async fn preflight(
        &self,
        destinations: &[(Destination, i64)],
    ) -> std::result::Result<(), BankFailure> {
        for (target, total) in destinations {
            let allocation = target.allocation.as_str();
            target.validate().map_err(|_| {
                preflight_failure(
                    allocation,
                    "configuration",
                    "Account or pot ID format is invalid. Check Configuration.",
                )
            })?;
            let kind = if target.allocation == "personal" {
                "uk_retail"
            } else {
                "uk_retail_joint"
            };
            let accounts = self
                .preflight_request(allocation, "/accounts", &[("account_type", kind.into())])
                .await?;
            let accounts = accounts["accounts"].as_array().ok_or_else(|| {
                preflight_failure(
                    allocation,
                    "accounts response",
                    "Monzo did not return an accounts list.",
                )
            })?;
            let account = accounts.iter().find(|a| a["id"] == target.account_id)
                .ok_or_else(|| preflight_failure(allocation,"account lookup","The saved account ID was not returned for this account type. Check the Personal/Joint allocation and the token's account access."))?;
            if account["closed"] == true {
                return Err(preflight_failure(
                    allocation,
                    "account status",
                    "The saved account is closed.",
                ));
            }
            let pots = self
                .preflight_request(
                    allocation,
                    "/pots",
                    &[("current_account_id", target.account_id.clone())],
                )
                .await?;
            let pots = pots["pots"].as_array().ok_or_else(|| {
                preflight_failure(
                    allocation,
                    "pots response",
                    "Monzo did not return a pots list.",
                )
            })?;
            let pot = pots.iter().find(|p| p["id"] == target.pot_id)
                .ok_or_else(|| preflight_failure(allocation,"pot lookup","The saved pot ID was not returned for the saved account. Check that the pot belongs to this account."))?;
            if pot["deleted"] != false {
                return Err(preflight_failure(
                    allocation,
                    "pot status",
                    "The pot is deleted or Monzo did not confirm its active status.",
                ));
            }
            if pot["currency"] != "GBP" {
                return Err(preflight_failure(
                    allocation,
                    "pot currency",
                    "Monzo did not report GBP as the pot currency.",
                ));
            }
            let balance = self
                .preflight_request(
                    allocation,
                    "/balance",
                    &[("account_id", target.account_id.clone())],
                )
                .await?;
            if balance["currency"] != "GBP" {
                return Err(preflight_failure(
                    allocation,
                    "account currency",
                    "Monzo did not report GBP as the account currency.",
                ));
            }
            let available = balance["balance"].as_i64().ok_or_else(|| {
                preflight_failure(
                    allocation,
                    "balance response",
                    "Monzo did not return a valid available account balance.",
                )
            })?;
            if available < *total {
                return Err(preflight_failure(
                    allocation,
                    "insufficient funds",
                    format!(
                        "The source account's available balance is below the £{} required for this allocation. Money already in pots is not part of the available account balance.",
                        crate::recon::format_pennies(*total)
                    ),
                ));
            }
        }
        Ok(())
    }
    async fn deposit(
        &self,
        operation: &Operation,
    ) -> std::result::Result<Option<String>, BankFailure> {
        let result = self
            .request(
                Method::PUT,
                &format!("/pots/{}/deposit", operation.destination.pot_id),
                &[
                    (
                        "source_account_id",
                        operation.destination.account_id.clone(),
                    ),
                    ("amount", operation.amount_pence.to_string()),
                    ("dedupe_id", operation.dedupe_id.clone()),
                ],
            )
            .await?;
        if result["id"] != operation.destination.pot_id || result["currency"] != "GBP" {
            return Err(BankFailure::Uncertain);
        }
        // The documented response is a pot. Use a transaction ID if Monzo also
        // supplies one; otherwise resolve the account's pot-transfer transaction.
        Ok(result["transaction_id"]
            .as_str()
            .or_else(|| result["transaction"]["id"].as_str())
            .filter(|id| crate::execution::valid_id(id, "tx_"))
            .map(str::to_owned))
    }
    async fn find_transactions(
        &self,
        operations: &[Operation],
        used: &BTreeSet<String>,
    ) -> std::result::Result<Vec<(i64, String)>, BankFailure> {
        for attempt in 0..5 {
            match self.discover_transactions(operations, used).await {
                Err(BankFailure::NoteMatchPending) if attempt < 4 => {
                    tokio::time::sleep(self.poll_delay).await
                }
                result => return result,
            }
        }
        Err(BankFailure::NoteMatchPending)
    }
    async fn annotate(
        &self,
        operation: &Operation,
        transaction_id: &str,
    ) -> std::result::Result<(), BankFailure> {
        let response = self
            .request(
                Method::PATCH,
                &format!("/transactions/{transaction_id}"),
                &[
                    ("metadata[notes]", operation.note.clone()),
                    ("metadata[recon_operation]", operation.dedupe_id.clone()),
                ],
            )
            .await?;
        if response["transaction"]["id"] != transaction_id {
            return Err(BankFailure::Uncertain);
        }
        Ok(())
    }
}
