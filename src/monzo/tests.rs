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
        ok(json!({"transactions":[{"id":"tx_test","currency":"GBP","amount":-1234}]})),
        ok(json!({"transaction":{"id":"tx_test"}})),
    ])
    .await;
    bank.preflight(&[(operation.destination.clone(), 1234)])
        .await
        .unwrap();
    bank.deposit(&operation).await.unwrap();
    bank.validate_note(&operation, "tx_test").await.unwrap();
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
    for replies in [
        vec![ok(json!({"accounts":[]}))],
        vec![ok(json!({"accounts":[{"id":"acc_test","closed":true}]}))],
        vec![
            ok(accounts.clone()),
            ok(json!({"pots":[{"id":"pot_test","currency":"EUR","deleted":false}]})),
        ],
        vec![
            ok(accounts.clone()),
            ok(pots.clone()),
            ok(json!({"currency":"GBP","balance":1233})),
        ],
        vec![
            ok(accounts),
            ok(pots),
            ok(json!({"currency":"EUR","balance":5000})),
        ],
    ] {
        let (bank, _server) = mock(replies).await;
        assert!(matches!(
            bank.preflight(&[(op().destination, 1234)]).await,
            Err(BankFailure::Rejected)
        ));
    }
}

#[tokio::test]
async fn note_matching_paginates_and_rejects_wrong_amount_or_declined_transaction() {
    let rows: Vec<_> = (0..100).map(|i| json!({"id":format!("tx_{i}")})).collect();
    let (bank, server) = mock(vec![
        ok(json!({"transactions":rows})),
        ok(json!({"transactions":[{"id":"tx_match","currency":"GBP","amount":-1234}]})),
    ])
    .await;
    bank.validate_note(&op(), "tx_match").await.unwrap();
    assert_eq!(
        server.script.lock().unwrap().calls[1].fields["since"],
        "tx_99"
    );
    for transaction in [
        json!({"id":"tx_match","currency":"GBP","amount":1234}),
        json!({"id":"tx_match","currency":"GBP","amount":-1234,"decline_reason":"declined"}),
        json!({"id":"tx_other","currency":"GBP","amount":-1234}),
    ] {
        let (bank, _server) = mock(vec![ok(json!({"transactions":[transaction]}))]).await;
        assert!(matches!(
            bank.validate_note(&op(), "tx_match").await,
            Err(BankFailure::Rejected)
        ));
    }
}
