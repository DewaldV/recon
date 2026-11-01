use recon::{
    Error,
    queue::{PageAccountEdit, PurchaseEdit},
    review::{Change, Filters},
    storage::Store,
};
use rusqlite::Connection;
use std::collections::BTreeSet;

fn apply(s: &mut Store, c: Change) {
    let t = s.stage_review(c).unwrap();
    s.commit_review(&t, &BTreeSet::new(), true).unwrap();
}
fn setup() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(d.path()).unwrap();
    s.import("synthetic.csv", include_bytes!("fixtures/virgin-first.csv"))
        .unwrap();
    apply(
        &mut s,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Synthetic groceries".into(),
        },
    );
    apply(
        &mut s,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: String::new(),
            account: "personal".into(),
        },
    );
    (d, s)
}
fn stage(s: &mut Store, ids: &[i64]) -> Change {
    Change::Staging {
        rows: s
            .detail_rows()
            .unwrap()
            .into_iter()
            .filter(|r| ids.contains(&r.id))
            .map(|r| (r.id, r.signature()))
            .collect(),
        staged: true,
    }
}

fn page_edits(store: &Store) -> Vec<PageAccountEdit> {
    store
        .detail_rows()
        .unwrap()
        .into_iter()
        .map(|r| PageAccountEdit {
            id: r.id,
            signature: r.signature(),
            original: r.account.clone(),
            account: r.account,
        })
        .collect()
}

#[test]
fn page_accounts_save_different_choices_and_preserve_unchanged_rule_provenance() {
    let (_dir, mut store) = setup();
    let c = stage(&mut store, &[1, 2]);
    apply(&mut store, c);
    let mut edits = page_edits(&store);
    edits.iter_mut().find(|e| e.id == 1).unwrap().account = "joint".into();
    assert_eq!(store.save_page_accounts(&edits).unwrap(), 1);
    let untouched = store.detail_row(2).unwrap();
    assert_eq!(untouched.version, 0);
    assert!(untouched.account_source.starts_with("account rule"));
    assert!(untouched.staged);
    assert!(!store.detail_row(1).unwrap().staged);
    let mut edits = page_edits(&store);
    edits
        .iter_mut()
        .find(|e| e.id == 1)
        .unwrap()
        .account
        .clear();
    edits.iter_mut().find(|e| e.id == 2).unwrap().account = "joint".into();
    assert_eq!(store.save_page_accounts(&edits).unwrap(), 2);
    assert_eq!(
        store.detail_row(1).unwrap().account,
        "personal",
        "clearing falls back to the rule"
    );
    assert_eq!(store.detail_row(2).unwrap().account, "joint");
    let revision = store.review_revision().unwrap();
    assert_eq!(store.save_page_accounts(&page_edits(&store)).unwrap(), 0);
    assert_eq!(store.review_revision().unwrap(), revision);
}

#[test]
fn stale_or_failed_page_save_rolls_back_every_changed_account() {
    let (dir, mut store) = setup();
    let mut edits = page_edits(&store);
    edits.sort_by_key(|e| e.id);
    for edit in &mut edits {
        edit.account = "joint".into();
    }
    store.decide(2, 0, "personal").unwrap();
    assert!(matches!(
        store.save_page_accounts(&edits),
        Err(Error::Stale)
    ));
    assert_eq!(store.detail_row(1).unwrap().version, 0);
    let mut edits = page_edits(&store);
    edits.sort_by_key(|e| e.id);
    for edit in &mut edits {
        edit.account = "joint".into();
    }
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_page BEFORE INSERT ON review_decisions WHEN NEW.purchase_id=2 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(store.save_page_accounts(&edits).is_err());
    assert_eq!(store.detail_row(1).unwrap().version, 0);
    assert_eq!(store.detail_row(2).unwrap().version, 1);
    db.execute_batch("DROP TRIGGER fail_page;").unwrap();
    apply(
        &mut store,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: String::new(),
            account: "joint".into(),
        },
    );
    assert!(
        matches!(store.save_page_accounts(&edits), Err(Error::Stale)),
        "rule changes invalidate pending edits"
    );
}

#[test]
fn import_autofills_but_never_stages_and_rules_respect_overrides() {
    let (_d, mut s) = setup();
    assert!(
        s.detail_rows()
            .unwrap()
            .iter()
            .all(|r| r.account == "personal" && r.handling == "unknown" && !r.staged)
    );
    apply(
        &mut s,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: "0000".into(),
            account: "joint".into(),
        },
    );
    assert!(
        s.detail_rows()
            .unwrap()
            .iter()
            .all(|r| r.account == "joint")
    );
    s.decide(1, 0, "personal").unwrap();
    apply(
        &mut s,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: "0000".into(),
            account: String::new(),
        },
    );
    let rows = s.detail_rows().unwrap();
    assert!(rows.iter().all(|r| r.account == "personal"));
    assert!(
        rows.iter()
            .find(|r| r.id == 1)
            .unwrap()
            .account_source
            .starts_with("purchase choice")
    );
}

#[test]
fn staging_is_versioned_global_and_invalidated_only_by_effective_changes() {
    let (d, mut s) = setup();
    let c = stage(&mut s, &[1, 2]);
    apply(&mut s, c);
    let q = s.queue_summary().unwrap();
    assert_eq!((q.staged, q.needs_review, q.personal_pence), (2, 0, 2468));
    assert!(!q.runnable(), "unresolved source row must block execution");
    let filtered = s
        .review_page(&Filters {
            q: "not present".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.total, 0);
    assert_eq!(filtered.queue.staged, 2);
    apply(
        &mut s,
        Change::Default {
            merchant: "Unrelated".into(),
            category: "Shopping".into(),
            description: "Unrelated".into(),
        },
    );
    assert_eq!(s.queue_summary().unwrap().staged, 2);
    s.import("repeat", include_bytes!("fixtures/virgin-first.csv"))
        .unwrap();
    assert_eq!(s.queue_summary().unwrap().staged, 2);
    apply(
        &mut s,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: String::new(),
            account: "joint".into(),
        },
    );
    assert_eq!(s.queue_summary().unwrap().staged, 0);
    let c = stage(&mut s, &[1, 2]);
    apply(&mut s, c);
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(s.queue_summary().unwrap().joint_pence, 2468);
}

#[test]
fn stale_page_and_stale_preview_cannot_stage_new_values() {
    let (_d, mut s) = setup();
    let c = stage(&mut s, &[1]);
    s.decide(1, 0, "joint").unwrap();
    assert!(matches!(s.stage_review(c), Err(Error::Stale)));
    let c = stage(&mut s, &[1]);
    let token = s.stage_review(c).unwrap();
    s.decide(2, 0, "joint").unwrap();
    assert!(matches!(
        s.commit_review(&token, &BTreeSet::new(), false),
        Err(Error::Stale)
    ));
    assert_eq!(s.queue_summary().unwrap().staged, 0);
}

#[test]
fn save_and_stage_is_atomic_and_cannot_reopen_handled_history() {
    let (_d, mut s) = setup();
    let row = s
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    let edit = |signature: String| PurchaseEdit {
        id: 1,
        signature,
        account: "joint".into(),
        category: "Shopping".into(),
        description: "Edited".into(),
        use_category_default: false,
        use_description_default: false,
        allow_category: false,
        stage: true,
    };
    s.edit_purchase(edit(row.signature())).unwrap();
    let row = s
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    assert!(row.staged);
    assert_eq!(row.description, "Edited");
    apply(
        &mut s,
        Change::Handling {
            ids: vec![1],
            state: "external".into(),
            reason: "Synthetic external funding evidence".into(),
            reopen: false,
        },
    );
    apply(
        &mut s,
        Change::Handling {
            ids: vec![1],
            state: "unknown".into(),
            reason: "Recheck".into(),
            reopen: false,
        },
    );
    let row = s
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    let mut changed = edit(row.signature());
    changed.description = "Must roll back".into();
    assert!(s.edit_purchase(changed).is_err());
    let row = s
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    assert_eq!(row.description, "Edited");
    assert!(!row.staged);
}

#[test]
fn migration_keeps_workspace_history_without_staging_it() {
    let d = tempfile::tempdir().unwrap();
    let db = Connection::open(d.path().join("recon.sqlite3")).unwrap();
    for sql in [
        include_str!("../migrations/0001_initial.sql"),
        include_str!("../migrations/0002_original_currency.sql"),
        include_str!("../migrations/0003_merchant_review.sql"),
        include_str!("../migrations/0004_recon_handling.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    db.execute_batch("INSERT INTO source_accounts VALUES(1,'virgin','virgin-primary'); INSERT INTO purchases VALUES(1,1,'synthetic','2026-01-01','2026-01-02',100,'GBP','SYNTHETIC SHOP','0000','GBP'); INSERT INTO recons(id,name,name_key) VALUES(1,'Old selection','old selection'); INSERT INTO recon_status_events(recon_id,version,status,reason) VALUES(1,1,'active','Old scope'); INSERT INTO recon_membership_events(recon_id,purchase_id,version,action,reason) VALUES(1,1,1,'add','Old selection'); PRAGMA user_version=4;").unwrap();
    drop(db);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(s.recon_members(1).unwrap().len(), 1);
    assert_eq!(s.queue_summary().unwrap().staged, 0);
    assert_eq!(s.queue_summary().unwrap().needs_review, 1);
}

#[test]
fn failed_bulk_stage_rolls_back_handling_and_staging() {
    let (d, mut s) = setup();
    let db = Connection::open(d.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_stage BEFORE INSERT ON staging_events WHEN NEW.purchase_id=2 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let c = stage(&mut s, &[1, 2]);
    let token = s.stage_review(c).unwrap();
    assert!(s.commit_review(&token, &BTreeSet::new(), false).is_err());
    assert!(
        s.detail_rows()
            .unwrap()
            .iter()
            .all(|r| r.handling == "unknown" && !r.staged)
    );
    assert!(s.review_preview(&token).is_ok());
}
