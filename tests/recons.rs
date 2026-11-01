use std::collections::BTreeSet;

use recon::{
    Error,
    review::{Change, Filters},
    storage::Store,
};
use rusqlite::{Connection, params};

const FIRST: &[u8] = include_bytes!("fixtures/virgin-first.csv");

fn apply(store: &mut Store, change: Change) {
    let token = store.stage_review(change).unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
}
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("synthetic.csv", FIRST).unwrap();
    apply(
        &mut store,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Synthetic purchase".into(),
        },
    );
    store.decide(1, 0, "personal").unwrap();
    store.decide(2, 0, "joint").unwrap();
    (dir, store)
}
fn create(store: &mut Store, name: &str) -> i64 {
    apply(store, Change::CreateRecon { name: name.into() });
    store
        .recons()
        .unwrap()
        .into_iter()
        .find(|r| r.name == name)
        .unwrap()
        .id
}
fn membership(ids: &[i64], recon_id: i64, include: bool) -> Change {
    Change::Membership {
        ids: ids.into(),
        recon_id,
        include,
        reason: "Explicit synthetic selection".into(),
    }
}
fn handling(ids: &[i64], state: &str, reopen: bool) -> Change {
    Change::Handling {
        ids: ids.into(),
        state: state.into(),
        reason: "Synthetic evidence: manual pot transfer checked".into(),
        reopen,
    }
}
fn status(id: i64, value: &str) -> Change {
    Change::ReconStatus {
        recon_id: id,
        status: value.into(),
        reason: "Synthetic workspace status change; not evidence of payment".into(),
    }
}

#[test]
fn complete_details_and_selection_do_not_infer_outstanding() {
    let (_dir, mut store) = setup();
    assert!(
        store.detail_rows().unwrap().iter().all(|r| r.complete()
            && !r.ready()
            && r.handling == "unknown"
            && r.handling_version == 0)
    );
    let id = create(&mut store, "Synthetic recon");
    assert_eq!(store.recon_summary(id).unwrap().total, 0);
    apply(&mut store, membership(&[1, 2], id, true));
    let summary = store.recon_summary(id).unwrap();
    assert_eq!((summary.total, summary.unknown, summary.ready), (2, 2, 0));
    assert_eq!(
        (
            summary.personal_amount.as_str(),
            summary.joint_amount.as_str()
        ),
        ("0.00", "0.00")
    );
    assert_eq!(
        store
            .review_page(&Filters {
                view: "ready".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        store
            .review_page(&Filters {
                view: "attention".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        2
    );
}

#[test]
fn outstanding_requires_active_scope_and_complete_fields() {
    let (_dir, mut store) = setup();
    apply(&mut store, handling(&[1, 2], "outstanding", false));
    assert!(store.detail_rows().unwrap().iter().all(|r| !r.ready()));
    let id = create(&mut store, "Scope");
    apply(&mut store, membership(&[1, 2], id, true));
    let summary = store.recon_summary(id).unwrap();
    assert_eq!(
        (summary.ready, summary.personal_count, summary.joint_count),
        (2, 1, 1)
    );
    assert_eq!(
        (
            summary.personal_amount.as_str(),
            summary.joint_amount.as_str()
        ),
        ("12.34", "12.34")
    );
    store.decide(1, 1, "").unwrap();
    let summary = store.recon_summary(id).unwrap();
    assert_eq!(
        (
            summary.ready,
            summary.blocked,
            summary.personal_amount.as_str()
        ),
        (1, 1, "0.00")
    );
    apply(&mut store, membership(&[2], id, false));
    assert_eq!(store.recon_summary(id).unwrap().total, 1);
    assert!(
        !store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 2)
            .unwrap()
            .ready()
    );
}

#[test]
fn external_and_excluded_are_global_and_survive_imports_restart_and_restore() {
    let (dir, mut store) = setup();
    let id = create(&mut store, "First recon");
    apply(&mut store, membership(&[1, 2], id, true));
    apply(&mut store, handling(&[1], "external", false));
    apply(&mut store, handling(&[2], "excluded", false));
    let summary = store.recon_summary(id).unwrap();
    assert_eq!(
        (summary.external, summary.excluded, summary.ready),
        (1, 1, 0)
    );
    store.import("again.csv", FIRST).unwrap();
    store
        .import("overlap.csv", include_bytes!("fixtures/virgin-overlap.csv"))
        .unwrap();
    apply(&mut store, status(id, "archived"));
    let next = create(&mut store, "Next recon");
    apply(&mut store, membership(&[1, 2], next, true));
    assert_eq!(store.recon_summary(next).unwrap().ready, 0);
    let snapshot = store.backup().unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.recon_summary(next).unwrap().external, 1);
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(snapshot, restored.path().join("recon.sqlite3")).unwrap();
    let store = Store::open(restored.path()).unwrap();
    assert_eq!(store.recon_summary(next).unwrap().excluded, 1);
    let history = store.purchase_history(1).unwrap();
    assert!(history.iter().any(|e| e.kind.contains("handling")
        && e.value.starts_with("external")
        && e.reason.contains("pot transfer")));
}

#[test]
fn handled_work_cannot_be_reopened_indirectly_through_unknown() {
    let (_dir, mut store) = setup();
    apply(&mut store, handling(&[1], "external", false));
    assert!(
        store
            .stage_review(handling(&[1], "outstanding", false))
            .is_err()
    );
    apply(&mut store, handling(&[1], "unknown", false));
    assert!(
        store
            .stage_review(handling(&[1], "outstanding", false))
            .is_err()
    );
    apply(&mut store, handling(&[1], "outstanding", true));
    let row = store
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    assert_eq!(
        (row.handling.as_str(), row.handling_version),
        ("outstanding", 3)
    );
    assert!(
        store
            .purchase_history(1)
            .unwrap()
            .iter()
            .any(|e| e.value.contains("reopening confirmed"))
    );
}

#[test]
fn duplicate_active_scope_is_blocked_including_reactivation() {
    let (_dir, mut store) = setup();
    let first = create(&mut store, "First");
    let second = create(&mut store, "Second");
    apply(&mut store, membership(&[1], first, true));
    assert!(
        store
            .stage_review(membership(&[1, 2], second, true))
            .is_err()
    );
    assert!(store.recon_members(second).unwrap().is_empty());
    apply(&mut store, status(first, "archived"));
    assert!(store.stage_review(membership(&[2], first, true)).is_err());
    apply(&mut store, membership(&[1], second, true));
    assert!(store.stage_review(status(first, "active")).is_err());
    apply(&mut store, membership(&[1], second, false));
    apply(&mut store, status(first, "active"));
    assert_eq!(store.recon_members(first).unwrap(), BTreeSet::from([1]));
}

#[test]
fn archiving_never_settles_work_and_archived_scope_cannot_appear_ready_elsewhere() {
    let (_dir, mut store) = setup();
    let first = create(&mut store, "First");
    apply(&mut store, membership(&[1], first, true));
    apply(&mut store, handling(&[1], "outstanding", false));
    apply(&mut store, status(first, "archived"));
    let old = store.recon_summary(first).unwrap();
    assert_eq!((old.ready, old.blocked, old.external), (0, 1, 0));
    let second = create(&mut store, "Second");
    apply(&mut store, membership(&[1], second, true));
    assert_eq!(store.recon_summary(second).unwrap().ready, 1);
    assert_eq!(store.recon_summary(first).unwrap().ready, 0);
    assert_eq!(
        store
            .review_page(&Filters {
                recon_id: Some(first),
                view: "ready".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        store
            .review_page(&Filters {
                recon_id: Some(second),
                view: "ready".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        1
    );
}

#[test]
fn reasons_bounds_invalid_states_missing_purchases_and_names_are_validated() {
    let (_dir, mut store) = setup();
    for change in [
        Change::CreateRecon { name: " ".into() },
        handling(&[], "external", false),
        handling(&[1, 1], "external", false),
        handling(&[999], "external", false),
        handling(&[1], "paid", false),
        Change::Handling {
            ids: vec![1],
            state: "external".into(),
            reason: " ".into(),
            reopen: false,
        },
        Change::Handling {
            ids: vec![1],
            state: "unknown".into(),
            reason: "a".repeat(4001),
            reopen: false,
        },
    ] {
        assert!(store.stage_review(change).is_err());
    }
    create(&mut store, "Test recon");
    assert!(
        store
            .stage_review(Change::CreateRecon {
                name: "  TEST   RECON ".into()
            })
            .is_err()
    );
    assert!(store.recon_summary(999).is_err());
    assert!(store.purchase_history(999).is_err());
    assert!(store.stage_review(membership(&[1], 999, true)).is_err());
}

#[test]
fn handling_and_membership_previews_are_stale_after_any_relevant_edit() {
    let (_dir, mut store) = setup();
    let recon = create(&mut store, "Scope");
    let token = store
        .stage_review(handling(&[1], "outstanding", false))
        .unwrap();
    store.decide(1, 1, "joint").unwrap();
    assert!(matches!(
        store.commit_review(&token, &BTreeSet::new(), false),
        Err(Error::Stale)
    ));
    let token = store.stage_review(membership(&[1], recon, true)).unwrap();
    apply(&mut store, handling(&[1], "external", false));
    assert!(matches!(
        store.commit_review(&token, &BTreeSet::new(), false),
        Err(Error::Stale)
    ));
    let token = store
        .stage_review(handling(&[1], "unknown", false))
        .unwrap();
    store
        .commit_review(&token, &BTreeSet::new(), false)
        .unwrap();
    assert!(matches!(
        store.commit_review(&token, &BTreeSet::new(), false),
        Err(Error::Stale)
    ));
}

#[test]
fn sql_guards_preserve_audit_history_and_active_scope() {
    let (dir, mut store) = setup();
    let first = create(&mut store, "First");
    let second = create(&mut store, "Second");
    apply(&mut store, handling(&[1], "external", false));
    apply(&mut store, membership(&[1], first, true));
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    for table in [
        "handling_decisions",
        "recons",
        "recon_membership_events",
        "recon_status_events",
    ] {
        assert!(db.execute(&format!("DELETE FROM {table}"), []).is_err());
    }
    assert!(
        db.execute("UPDATE handling_decisions SET state='outstanding'", [])
            .is_err()
    );
    assert!(db.execute("INSERT INTO handling_decisions(purchase_id,version,state,reason,reopen_confirmed,provenance) VALUES(1,2,'outstanding','No confirmation',0,'manual')",[]).is_err());
    assert!(db.execute("INSERT INTO recon_membership_events(recon_id,purchase_id,version,action,reason) VALUES(?1,1,1,'add','Duplicate')",[second]).is_err());
}

#[test]
fn failed_bulk_handling_commit_rolls_back_every_row_and_preserves_preview() {
    let (dir, mut store) = setup();
    let token = store
        .stage_review(handling(&[1, 2], "external", false))
        .unwrap();
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER synthetic_failure BEFORE INSERT ON handling_decisions WHEN NEW.purchase_id=2 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(
        store
            .commit_review(&token, &BTreeSet::new(), false)
            .is_err()
    );
    assert!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .all(|r| r.handling == "unknown")
    );
    assert!(store.review_preview(&token).is_ok());
    db.execute_batch("DROP TRIGGER synthetic_failure;").unwrap();
    store
        .commit_review(&token, &BTreeSet::new(), false)
        .unwrap();
}

#[test]
fn source_conflicts_remain_blocked_and_status_counts_partition_selection() {
    let (_dir, mut store) = setup();
    let id = create(&mut store, "Scope");
    apply(&mut store, membership(&[1, 2], id, true));
    apply(&mut store, handling(&[1, 2], "outstanding", false));
    let changed = String::from_utf8(FIRST.into())
        .unwrap()
        .replace("12.34", "99.00");
    store.import("conflict.csv", changed.as_bytes()).unwrap();
    let result = store.recon_summary(id).unwrap();
    assert_eq!((result.ready, result.blocked), (0, 2));
    apply(&mut store, handling(&[1], "external", false));
    let result = store.recon_summary(id).unwrap();
    assert_eq!(
        result.total,
        result.ready + result.blocked + result.unknown + result.external + result.excluded
    );
    assert_eq!((result.external, result.blocked), (1, 1));
    assert_eq!(result.personal_amount, "0.00");
}

#[test]
fn totals_reject_overflow_rather_than_wrap_or_silently_omit_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let bytes = String::from_utf8(FIRST.into())
        .unwrap()
        .replace("12.34", "92233720368547758.07");
    store
        .import("large-synthetic.csv", bytes.as_bytes())
        .unwrap();
    apply(
        &mut store,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Synthetic".into(),
        },
    );
    store.decide(1, 0, "personal").unwrap();
    store.decide(2, 0, "personal").unwrap();
    let id = create(&mut store, "Overflow test");
    apply(&mut store, membership(&[1, 2], id, true));
    apply(&mut store, handling(&[1, 2], "outstanding", false));
    assert!(matches!(store.recon_summary(id), Err(Error::Invalid(_))));
}

#[test]
fn schema_three_upgrade_keeps_choices_and_defaults_all_handling_to_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0002_original_currency.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0003_merchant_review.sql"))
        .unwrap();
    db.execute_batch("INSERT INTO source_accounts VALUES(1,'virgin','virgin-primary'); INSERT INTO purchases(id,source_account_id,reference,transaction_date,posting_date,amount_pence,currency,merchant,card_used,original_currency) VALUES(1,1,'synthetic','2026-01-01','2026-01-02',100,'GBP','Shop','0000','GBP'); INSERT INTO review_decisions(purchase_id,version,account) VALUES(1,1,'joint'); PRAGMA user_version=3;").unwrap();
    drop(db);
    let store = Store::open(dir.path()).unwrap();
    let row = store.detail_rows().unwrap().remove(0);
    assert_eq!(
        (
            row.account.as_str(),
            row.handling.as_str(),
            row.handling_version
        ),
        ("joint", "unknown", 0)
    );
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM handling_decisions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .count(),
        1
    );
    assert!(
        db.query_row(
            "SELECT state FROM current_handling WHERE purchase_id=?1",
            params![1],
            |r| r.get::<_, String>(0)
        )
        .is_err()
    );
}
