//! Backend-only temporary credentials. No remote error body is surfaced or logged.
use crate::execution::{Bank, BankFailure, Destination, Operation};
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use std::time::Duration;
use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

pub struct Monzo {
    client: Client,
    token: Zeroizing<String>,
    base_url: String,
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
        })
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        fields: &[(&str, String)],
    ) -> std::result::Result<Value, BankFailure> {
        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.base_url))
            .bearer_auth(self.token.as_str());
        request = if method == Method::GET {
            request.query(fields)
        } else {
            request.form(fields)
        };
        let response = request.send().await.map_err(|_| BankFailure::Uncertain)?;
        match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(BankFailure::Unauthorized);
            }
            StatusCode::TOO_MANY_REQUESTS => return Err(BankFailure::RateLimited),
            StatusCode::REQUEST_TIMEOUT | StatusCode::CONFLICT => {
                return Err(BankFailure::Uncertain);
            }
            s if s.is_client_error() => return Err(BankFailure::Rejected),
            s if !s.is_success() => return Err(BankFailure::Uncertain),
            _ => {}
        }
        response.json().await.map_err(|_| BankFailure::Uncertain)
    }
}
impl Bank for Monzo {
    async fn preflight(
        &self,
        destinations: &[(Destination, i64)],
    ) -> std::result::Result<(), BankFailure> {
        for (target, total) in destinations {
            target.validate().map_err(|_| BankFailure::Rejected)?;
            let kind = if target.allocation == "personal" {
                "uk_retail"
            } else {
                "uk_retail_joint"
            };
            let accounts = self
                .request(Method::GET, "/accounts", &[("account_type", kind.into())])
                .await?;
            if !accounts["accounts"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|a| a["id"] == target.account_id && a["closed"] != true)
            }) {
                return Err(BankFailure::Rejected);
            }
            let pots = self
                .request(
                    Method::GET,
                    "/pots",
                    &[("current_account_id", target.account_id.clone())],
                )
                .await?;
            if !pots["pots"].as_array().is_some_and(|p| {
                p.iter().any(|p| {
                    p["id"] == target.pot_id && p["currency"] == "GBP" && p["deleted"] == false
                })
            }) {
                return Err(BankFailure::Rejected);
            }
            let balance = self
                .request(
                    Method::GET,
                    "/balance",
                    &[("account_id", target.account_id.clone())],
                )
                .await?;
            if balance["currency"] != "GBP"
                || balance["balance"].as_i64().is_none_or(|b| b < *total)
            {
                return Err(BankFailure::Rejected);
            }
        }
        Ok(())
    }
    async fn deposit(&self, operation: &Operation) -> std::result::Result<(), BankFailure> {
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
        Ok(())
    }
    async fn validate_note(
        &self,
        operation: &Operation,
        transaction_id: &str,
    ) -> std::result::Result<(), BankFailure> {
        // Verify account membership through the account-scoped, paginated endpoint.
        // The user supplies the mapping; equal amounts are validation, never automatic correlation.
        let mut since = (chrono::DateTime::parse_from_rfc3339(&operation.started_at)
            .map_err(|_| BankFailure::Rejected)?
            - chrono::Duration::minutes(1))
        .to_rfc3339();
        let mut found = false;
        for _ in 0..1000 {
            let mut fields = vec![
                ("account_id", operation.destination.account_id.clone()),
                ("limit", "100".into()),
            ];
            if !since.is_empty() {
                fields.push(("since", since.clone()));
            }
            let page = self.request(Method::GET, "/transactions", &fields).await?;
            let rows = page["transactions"]
                .as_array()
                .ok_or(BankFailure::Uncertain)?;
            if let Some(tx) = rows.iter().find(|t| t["id"] == transaction_id) {
                if tx["currency"] != "GBP"
                    || tx["amount"].as_i64() != Some(-operation.amount_pence)
                    || tx
                        .get("decline_reason")
                        .is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                {
                    return Err(BankFailure::Rejected);
                }
                found = true;
                break;
            }
            if rows.len() < 100 {
                break;
            }
            let next = rows
                .last()
                .and_then(|t| t["id"].as_str())
                .ok_or(BankFailure::Uncertain)?;
            if next == since {
                return Err(BankFailure::Uncertain);
            }
            since = next.into();
        }
        if !found {
            return Err(BankFailure::Rejected);
        }
        Ok(())
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
