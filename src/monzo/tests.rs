use super::*;
use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    response::IntoResponse,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct Call {
    method: String,
    path: String,
    fields: BTreeMap<String, String>,
    authorization: String,
}
#[derive(Default)]
struct Script {
    replies: VecDeque<(u16, String)>,
    calls: Vec<Call>,
}
struct Server {
    task: tokio::task::JoinHandle<()>,
    script: Arc<Mutex<Script>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn mock(replies: Vec<(u16, String)>) -> (Monzo, Server) {
    async fn handle(
        State(state): State<Arc<Mutex<Script>>>,
        request: Request,
    ) -> impl IntoResponse {
        let method = request.method().to_string();
        let path = request.uri().path().to_owned();
        let query = request.uri().query().unwrap_or("").to_owned();
        let authorization = request.headers()["authorization"]
            .to_str()
            .unwrap()
            .to_owned();
        let body = to_bytes(request.into_body(), 100_000).await.unwrap();
        let fields = serde_urlencoded::from_bytes(if method == "GET" {
            query.as_bytes()
        } else {
            &body
        })
        .unwrap();
        let mut state = state.lock().unwrap();
        state.calls.push(Call {
            method,
            path,
            fields,
            authorization,
        });
        let (status, body) = state.replies.pop_front().expect("unexpected HTTP request");
        (
            axum::http::StatusCode::from_u16(status).unwrap(),
            [("content-type", "application/json")],
            body,
        )
    }
    let script = Arc::new(Mutex::new(Script {
        replies: replies.into(),
        ..Default::default()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().fallback(handle).with_state(script.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (
        Monzo {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .unwrap(),
            token: Zeroizing::new("synthetic-token".into()),
            base_url,
            poll_delay: Duration::ZERO,
        },
        Server { task, script },
    )
}
fn op() -> Operation {
    Operation {
        id: 1,
        run_id: 1,
        purchase_id: 1,
        destination: Destination {
            allocation: "joint".into(),
            account_id: "acc_test".into(),
            pot_id: "pot_test".into(),
        },
        amount_pence: 1234,
        note: "Groceries & lunch + tea".into(),
        merchant: "Synthetic".into(),
        dedupe_id: "persisted_key".into(),
        deposit: "pending".into(),
        note_status: "pending".into(),
        message: String::new(),
        started_at: "2026-10-01T12:00:00Z".into(),
        confirmed_at: "2026-10-01T12:00:02Z".into(),
        mapped_transaction: String::new(),
    }
}
fn ok(value: Value) -> (u16, String) {
    (200, value.to_string())
}

#[tokio::test]
async fn preflight_deposit_and_annotation_use_scoped_encoded_requests() {
    let operation = op();
    let (bank, server) = mock(vec![
        ok(json!({"accounts":[{"id":"acc_test","closed":false}]})),
        ok(json!({"pots":[{"id":"pot_test","currency":"GBP","deleted":false}]})),
        ok(json!({"currency":"GBP","balance":1234})),
        ok(json!({"id":"pot_test","currency":"GBP"})),
        ok(json!({"transactions":[{"id":"tx_test","currency":"GBP","amount":-1234,"created":"2026-10-01T12:00:01Z","metadata":{"pot_id":"pot_test"}}]})),
        ok(json!({"transaction":{"id":"tx_test"}})),
    ])
    .await;
    bank.preflight(&[(operation.destination.clone(), 1234)])
        .await
        .unwrap();
    bank.deposit(&operation).await.unwrap();
    let mappings = bank
        .find_transactions(std::slice::from_ref(&operation), &BTreeSet::new())
        .await
        .unwrap();
    assert_eq!(mappings, vec![(operation.id, "tx_test".into())]);
    bank.annotate(&operation, "tx_test").await.unwrap();
    let state = server.script.lock().unwrap();
    assert!(state.replies.is_empty());
    assert!(
        state
            .calls
            .iter()
            .all(|c| c.authorization == "Bearer synthetic-token")
    );
    assert_eq!(state.calls[0].fields["account_type"], "uk_retail_joint");
    assert_eq!(state.calls[1].fields["current_account_id"], "acc_test");
    assert_eq!(state.calls[2].fields["account_id"], "acc_test");
    let deposit = &state.calls[3];
    assert_eq!(
        (&*deposit.method, &*deposit.path),
        ("PUT", "/pots/pot_test/deposit")
    );
    assert_eq!(deposit.fields["amount"], "1234");
    assert_eq!(deposit.fields["source_account_id"], "acc_test");
    assert_eq!(deposit.fields["dedupe_id"], "persisted_key");
    assert_eq!(state.calls[4].fields["account_id"], "acc_test");
    assert_eq!(state.calls[5].method, "PATCH");
    assert_eq!(state.calls[5].fields["metadata[notes]"], operation.note);
}

#[tokio::test]
async fn failures_are_classified_without_exposing_remote_bodies() {
    for (status, expected) in [
        (400, "Rejected"),
        (401, "Unauthorized"),
        (403, "Unauthorized"),
        (408, "Uncertain"),
        (409, "Uncertain"),
        (429, "RateLimited"),
        (500, "Uncertain"),
        (302, "Uncertain"),
    ] {
        let (bank, _server) = mock(vec![(status, "secret remote body".into())]).await;
        let error = bank.deposit(&op()).await.unwrap_err();
        assert_eq!(format!("{error:?}"), expected);
        assert!(!error.message().contains("secret"));
    }
    for body in ["not json", "{}", r#"{"id":"pot_wrong","currency":"GBP"}"#] {
        let (bank, _server) = mock(vec![(200, body.into())]).await;
        assert!(matches!(
            bank.deposit(&op()).await,
            Err(BankFailure::Uncertain)
        ));
    }
}

#[tokio::test]
async fn preflight_rejects_wrong_account_pot_currency_and_insufficient_funds() {
    let accounts = json!({"accounts":[{"id":"acc_test","closed":false}]});
    let pots = json!({"pots":[{"id":"pot_test","currency":"GBP","deleted":false}]});
    for (check, replies) in [
        ("accounts response", vec![ok(json!({}))]),
        ("account lookup", vec![ok(json!({"accounts":[]}))]),
        (
            "account status",
            vec![ok(json!({"accounts":[{"id":"acc_test","closed":true}]}))],
        ),
        ("pots response", vec![ok(accounts.clone()), ok(json!({}))]),
        (
            "pot lookup",
            vec![ok(accounts.clone()), ok(json!({"pots":[]}))],
        ),
        (
            "pot status",
            vec![
                ok(accounts.clone()),
                ok(json!({"pots":[{"id":"pot_test","currency":"GBP","deleted":true}]})),
            ],
        ),
        (
            "pot currency",
            vec![
                ok(accounts.clone()),
                ok(json!({"pots":[{"id":"pot_test","currency":"EUR","deleted":false}]})),
            ],
        ),
        (
            "insufficient funds",
            vec![
                ok(accounts.clone()),
                ok(pots.clone()),
                ok(json!({"currency":"GBP","balance":1233})),
            ],
        ),
        (
            "balance response",
            vec![
                ok(accounts.clone()),
                ok(pots.clone()),
                ok(json!({"currency":"GBP"})),
            ],
        ),
        (
            "account currency",
            vec![
                ok(accounts),
                ok(pots),
                ok(json!({"currency":"EUR","balance":5000})),
            ],
        ),
    ] {
        let (bank, server) = mock(replies).await;
        let error = bank
            .preflight(&[(op().destination, 1234)])
            .await
            .unwrap_err();
        assert!(matches!(error, BankFailure::Preflight(_)));
        assert!(
            error
                .message()
                .contains(&format!("Joint preflight failed ({check})")),
            "{}",
            error.message()
        );
        if check == "insufficient funds" {
            assert!(error.message().contains("£12.34"));
        }
        assert!(
            server
                .script
                .lock()
                .unwrap()
                .calls
                .iter()
                .all(|c| c.method == "GET")
        );
    }
}

#[tokio::test]
async fn preflight_http_errors_identify_endpoint_and_status_without_remote_secrets() {
    for (path, status, prefix) in [
        ("/accounts", 400, vec![]),
        (
            "/pots",
            403,
            vec![ok(json!({"accounts":[{"id":"acc_test"}]}))],
        ),
        (
            "/balance",
            429,
            vec![
                ok(json!({"accounts":[{"id":"acc_test"}]})),
                ok(json!({"pots":[{"id":"pot_test","currency":"GBP","deleted":false}]})),
            ],
        ),
    ] {
        let mut replies = prefix;
        replies.push((
            status,
            "secret remote body synthetic-token acc_test pot_test".into(),
        ));
        let (bank, server) = mock(replies).await;
        let mut destination = op().destination;
        destination.allocation = "personal".into();
        let error = bank.preflight(&[(destination, 1234)]).await.unwrap_err();
        let message = error.message();
        assert!(message.contains(&format!("Personal preflight failed ({path})")));
        assert!(message.contains(&format!("HTTP {status}")));
        for secret in [
            "secret remote body",
            "synthetic-token",
            "acc_test",
            "pot_test",
        ] {
            assert!(!message.contains(secret));
        }
        assert!(
            server
                .script
                .lock()
                .unwrap()
                .calls
                .iter()
                .all(|c| c.method == "GET")
        );
    }
    let (bank, _server) = mock(vec![(200, "secret invalid JSON".into())]).await;
    let error = bank
        .preflight(&[(op().destination, 1234)])
        .await
        .unwrap_err();
    assert!(error.message().contains("HTTP 200") && error.message().contains("unreadable JSON"));
    assert!(!error.message().contains("secret"));
}

#[tokio::test]
async fn preflight_identifies_second_allocation_without_sending_deposits() {
    let account = ok(json!({"accounts":[{"id":"acc_test"}]}));
    let pot = ok(json!({"pots":[{"id":"pot_test","currency":"GBP","deleted":false}]}));
    let (bank, server) = mock(vec![
        account.clone(),
        pot.clone(),
        ok(json!({"currency":"GBP","balance":1234})),
        account,
        pot,
        ok(json!({"currency":"GBP","balance":1233,"total_balance":999999})),
    ])
    .await;
    let joint = op().destination;
    let mut personal = joint.clone();
    personal.allocation = "personal".into();
    let error = bank
        .preflight(&[(personal, 1234), (joint, 1234)])
        .await
        .unwrap_err();
    assert!(
        error
            .message()
            .contains("Joint preflight failed (insufficient funds)")
    );
    let script = server.script.lock().unwrap();
    assert_eq!(script.calls.len(), 6);
    assert!(script.calls.iter().all(|c| c.method == "GET"));
}

#[tokio::test]
async fn note_matching_paginates() {
    let rows: Vec<_> = (0..100).map(|i| json!({"id":format!("tx_{i}")})).collect();
    let (bank, server) = mock(vec![
        ok(json!({"transactions":rows})),
        ok(json!({"transactions":[transaction("tx_match","2026-10-01T12:00:01Z")]})),
    ])
    .await;
    assert_eq!(
        bank.find_transactions(&[op()], &BTreeSet::new())
            .await
            .unwrap(),
        vec![(1, "tx_match".into())]
    );
    assert_eq!(
        server.script.lock().unwrap().calls[1].fields["since"],
        "tx_99"
    );
}

fn transaction(id: &str, created: &str) -> Value {
    json!({"id":id,"created":created,"currency":"GBP","amount":-1234,"metadata":{"pot_id":"pot_test"}})
}

#[test]
fn automatic_matching_rejects_unrelated_or_occupied_transactions() {
    let operation = op();
    let original = transaction("tx_match", "2026-10-01T12:00:01Z");
    for (path, value) in [
        ("/amount", json!(1234)),
        ("/amount", json!(-1235)),
        ("/currency", json!("EUR")),
        ("/metadata/pot_id", json!("pot_other")),
        ("/created", json!("2026-10-01T11:59:00Z")),
        ("/created", json!("2026-10-01T12:00:08Z")),
        ("/created", json!("invalid")),
        ("/id", json!("bad_id")),
    ] {
        let mut tx = original.clone();
        *tx.pointer_mut(path).unwrap() = value;
        assert!(
            matches!(
                matching::match_transactions(&[&operation], &[tx], &BTreeSet::new()),
                Err(BankFailure::NoteMatchPending)
            ),
            "{path}"
        );
    }
    for (field, value) in [
        ("decline_reason", json!("declined")),
        ("account_id", json!("acc_other")),
        ("notes", json!("Someone else's note")),
    ] {
        let mut tx = original.clone();
        tx[field] = value;
        assert!(matching::match_transactions(&[&operation], &[tx], &BTreeSet::new()).is_err());
    }
    for (field, value) in [
        ("recon_operation", "another_operation"),
        ("notes", "Someone else's note"),
    ] {
        let mut tx = original.clone();
        tx["notes"] = json!("");
        tx["metadata"][field] = json!(value);
        assert!(matching::match_transactions(&[&operation], &[tx], &BTreeSet::new()).is_err());
    }
    assert!(
        matching::match_transactions(
            &[&operation],
            &[original],
            &BTreeSet::from(["tx_match".into()])
        )
        .is_err()
    );
}

#[test]
fn legacy_equal_amounts_match_chronologically_and_ambiguity_stays_pending() {
    let first = op();
    let mut second = first.clone();
    second.id = 2;
    second.dedupe_id = "second_key".into();
    second.note = "Second purchase".into();
    second.started_at = "2026-10-01T12:00:02Z".into();
    second.confirmed_at = "2026-10-01T12:00:04Z".into();
    let a = transaction("tx_first", "2026-10-01T12:00:01Z");
    let b = transaction("tx_second", "2026-10-01T12:00:03Z");
    let ops = [&second, &first];
    let used = BTreeSet::new();
    assert_eq!(
        matching::match_transactions(&ops, &[b.clone(), a.clone()], &used).unwrap(),
        vec![(1, "tx_first".into()), (2, "tx_second".into())]
    );
    for rows in [
        vec![a.clone()],
        vec![
            a.clone(),
            b.clone(),
            transaction("tx_extra", "2026-10-01T12:00:02Z"),
        ],
        vec![a.clone(), transaction("tx_tied", "2026-10-01T12:00:01Z")],
    ] {
        assert!(matches!(
            matching::match_transactions(&ops, &rows, &used),
            Err(BankFailure::NoteMatchPending)
        ));
    }
    let mut marked_a = a;
    marked_a["metadata"]["recon_operation"] = json!(second.dedupe_id);
    let mut marked_b = b;
    marked_b["metadata"]["recon_operation"] = json!(first.dedupe_id);
    assert_eq!(
        matching::match_transactions(&ops, &[marked_a, marked_b], &used).unwrap(),
        vec![(1, "tx_second".into()), (2, "tx_first".into())]
    );
}

#[tokio::test]
async fn discovery_polls_delayed_transactions_and_bounds_missing_matches() {
    let empty = ok(json!({"transactions":[]}));
    let (bank, server) = mock(vec![
        empty.clone(),
        empty.clone(),
        ok(json!({"transactions":[transaction("tx_visible","2026-10-01T12:00:01Z")]})),
    ])
    .await;
    assert_eq!(
        bank.find_transactions(&[op()], &BTreeSet::new())
            .await
            .unwrap(),
        vec![(1, "tx_visible".into())]
    );
    let calls = server.script.lock().unwrap().calls.clone();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|c| c.fields["account_id"] == "acc_test"
        && c.fields["since"] == "2026-10-01T11:59:55+00:00"
        && c.fields["before"] == "2026-10-01T12:00:07+00:00"));
    let (bank, server) = mock(vec![empty; 5]).await;
    assert!(matches!(
        bank.find_transactions(&[op()], &BTreeSet::new()).await,
        Err(BankFailure::NoteMatchPending)
    ));
    assert_eq!(server.script.lock().unwrap().calls.len(), 5);
}

#[tokio::test]
async fn discovery_scopes_equal_amounts_to_each_account() {
    let first = op();
    let mut second = op();
    second.id = 2;
    second.destination.account_id = "acc_zother".into();
    let (bank, server) = mock(vec![
        ok(json!({"transactions":[transaction("tx_first","2026-10-01T12:00:01Z")]})),
        ok(json!({"transactions":[transaction("tx_second","2026-10-01T12:00:01Z")]})),
    ])
    .await;
    assert_eq!(
        bank.find_transactions(&[first, second], &BTreeSet::new())
            .await
            .unwrap(),
        vec![(1, "tx_first".into()), (2, "tx_second".into())]
    );
    let calls = &server.script.lock().unwrap().calls;
    assert_eq!(calls[0].fields["account_id"], "acc_test");
    assert_eq!(calls[1].fields["account_id"], "acc_zother");
}

#[tokio::test]
async fn deposit_uses_returned_transaction_ids_when_available() {
    for (extra, expected) in [
        (json!({}), None),
        (json!({"transaction_id":"tx_returned"}), Some("tx_returned")),
        (json!({"transaction":{"id":"tx_nested"}}), Some("tx_nested")),
        (json!({"transaction_id":"invalid"}), None),
    ] {
        let mut response = json!({"id":"pot_test","currency":"GBP"});
        response
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (bank, _server) = mock(vec![ok(response)]).await;
        assert_eq!(bank.deposit(&op()).await.unwrap().as_deref(), expected);
    }
}
