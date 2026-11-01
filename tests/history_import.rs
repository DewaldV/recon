use recon::{Error, review::Change, storage::Store};
use rusqlite::Connection;
use std::collections::BTreeSet;

const CSV: &str = "Reference,Account,Category,Description,Paid,Reason\n00000000000000000000001,Personal,Shopping,Old note,Yes,Verified bank record\n00000000000000000000002,Joint,Shopping,Other note,No,Verified source reference\n";
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .import("synthetic", include_bytes!("fixtures/virgin-first.csv"))
        .unwrap();
    (dir, store)
}
fn preview(store: &mut Store, csv: &str) -> String {
    let change = store
        .parse_history("history.csv".into(), csv.as_bytes().to_vec())
        .unwrap();
    store.stage_review(change).unwrap()
}
fn apply(store: &mut Store, change: Change) {
    let token = store.stage_review(change).unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
}

#[test]
fn verified_history_records_provenance_without_staging_or_inferring_outstanding() {
    let (dir, mut store) = setup();
    let token = preview(&mut store, CSV);
    assert_eq!(store.review_preview(&token).unwrap().historical.len(), 2);
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
    let rows = store.detail_rows().unwrap();
    let paid = rows.iter().find(|r| r.id == 1).unwrap();
    assert_eq!(
        (&*paid.account, &*paid.handling, &*paid.description),
        ("personal", "external", "Old note")
    );
    let unpaid = rows.iter().find(|r| r.id == 2).unwrap();
    assert_eq!((&*unpaid.account, &*unpaid.handling), ("joint", "unknown"));
    assert!(rows.iter().all(|r| !r.staged));
    assert!(
        store
            .purchase_history(1)
            .unwrap()
            .iter()
            .any(|e| e.kind == "historical import" && e.reason.contains("Verified bank record"))
    );
    let token = preview(&mut store, CSV);
    assert!(store.review_preview(&token).unwrap().duplicate);
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 1)
            .unwrap()
            .handling_version,
        1
    );
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    let bytes: Vec<u8> = db
        .query_row("SELECT bytes FROM historical_sources", [], |r| r.get(0))
        .unwrap();
    assert_eq!(bytes, CSV.as_bytes());
    assert_eq!(
        db.query_row("SELECT count(*) FROM historical_matches", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn existing_decisions_including_cleared_overrides_win() {
    let (_dir, mut store) = setup();
    store.decide(1, 0, "").unwrap();
    apply(
        &mut store,
        Change::Details {
            ids: vec![1],
            category: "".into(),
            description: "".into(),
        },
    );
    apply(
        &mut store,
        Change::Handling {
            ids: vec![1],
            state: "unknown".into(),
            reason: "Explicit recheck".into(),
            reopen: false,
        },
    );
    let token = preview(&mut store, CSV);
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
    let row = store
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    assert!(row.account.is_empty() && row.category.is_empty() && row.description.is_empty());
    assert_eq!(row.handling, "unknown");
    assert_eq!(
        (row.version, row.details_version, row.handling_version),
        (1, 1, 1)
    );
}

#[test]
fn malformed_duplicate_and_unmatched_history_is_rejected() {
    let (_dir, store) = setup();
    for csv in [
        CSV.replace("00000000000000000000002", "00000000000000000000001"),
        CSV.replace("00000000000000000000001", "unmatched"),
        CSV.replace(",Yes,", ",Maybe,"),
        CSV.replace("Verified bank record", ""),
        CSV.replace("Personal", "Other"),
        CSV.replace("Reference,", "Unknown,"),
    ] {
        assert!(
            store
                .parse_history("history".into(), csv.into_bytes())
                .is_err()
        );
    }
    assert!(store.parse_history("history".into(), vec![255]).is_err());
}

#[test]
fn historical_preview_staleness_and_failure_roll_back_all_rows() {
    let (dir, mut store) = setup();
    let token = preview(&mut store, CSV);
    store.decide(2, 0, "personal").unwrap();
    assert!(matches!(
        store.commit_review(&token, &BTreeSet::new(), true),
        Err(Error::Stale)
    ));
    let token = preview(&mut store, CSV);
    assert!(
        store
            .commit_review(&token, &BTreeSet::new(), false)
            .is_err(),
        "new category needs approval"
    );
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_history BEFORE INSERT ON historical_matches WHEN NEW.purchase_id=2 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(store.commit_review(&token, &BTreeSet::new(), true).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM historical_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 1)
            .unwrap()
            .version,
        0
    );
    db.execute_batch("DROP TRIGGER fail_history;").unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
}
