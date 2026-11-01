use recon::{Error, execution::Destination, review::Change, storage::Store};
use rusqlite::Connection;
use std::collections::BTreeSet;

const DATE: &str = "2026-09-27";
fn csv(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut csv = include_str!("fixtures/virgin-first.csv")
        .lines()
        .next()
        .unwrap()
        .to_owned();
    csv.push('\n');
    for (reference, posting) in rows {
        csv.push_str(&format!("2026-09-25,{posting},12.34,SYNTHETIC SHOP,TEST CITY,GBR,TEST,{reference},DBIT,PR,BILLED,GBP,false,0000\n"));
    }
    csv.into_bytes()
}
fn apply(store: &mut Store, change: Change) {
    let token = store.stage_review(change).unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
}
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .import(
            "synthetic.csv",
            &csv(&[
                ("old", "2026-09-26"),
                ("boundary", DATE),
                ("new", "2026-09-28"),
                ("", "2026-09-26"),
            ]),
        )
        .unwrap();
    apply(
        &mut store,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Synthetic note".into(),
        },
    );
    apply(
        &mut store,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: String::new(),
            account: "personal".into(),
        },
    );
    store.decide(2, 0, "joint").unwrap();
    let rows = store
        .detail_rows()
        .unwrap()
        .iter()
        .map(|r| (r.id, r.signature()))
        .collect();
    apply(&mut store, Change::Staging { rows, staged: true });
    (dir, store)
}

#[test]
fn cutover_preserves_boundary_and_newer_choices_and_staging_and_runs_only_those() {
    let (dir, mut store) = setup();
    let before = store.detail_rows().unwrap();
    let stale = store
        .stage_review(Change::Accounts {
            rows: vec![(2, 1)],
            account: "personal".into(),
        })
        .unwrap();
    let result = store
        .set_cutover(
            DATE,
            "Historical spreadsheets are paid up before this posting date",
        )
        .unwrap();
    assert_eq!((result.excluded, result.resolved_exceptions), (1, 1));
    let backup = Connection::open(result.backup).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT count(*) FROM recon_cutover", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(matches!(store.review_preview(&stale), Err(Error::Stale)));
    let old = store.detail_row(1).unwrap();
    assert_eq!(old.handling, "excluded");
    assert!(!old.outstanding() && !old.staged && !old.can_stage());
    assert!(old.handling_reason.contains(DATE));
    assert!(matches!(
        store.stage_review(Change::Handling {
            ids: vec![1],
            state: "outstanding".into(),
            reason: "Attempt reopening".into(),
            reopen: true,
        }),
        Err(Error::Invalid(_))
    ));
    for prior in before.iter().filter(|r| r.id != 1) {
        let after = store.detail_row(prior.id).unwrap();
        assert_eq!(prior.signature(), after.signature());
        assert!(after.staged);
    }
    let summary = store.queue_summary().unwrap();
    assert_eq!(
        (summary.outstanding, summary.staged, summary.exceptions),
        (2, 2, 0)
    );
    assert!(summary.runnable());
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    assert!(db.execute("INSERT INTO handling_decisions(purchase_id,version,state,reason,reopen_confirmed,provenance) VALUES(1,3,'outstanding','Attempt reopening',1,'manual')",[]).is_err());
    assert!(db.execute("DELETE FROM recon_cutover", []).is_err());
    let run = store
        .prepare_run(
            store.review_revision().unwrap(),
            &[
                Destination {
                    allocation: "personal".into(),
                    account_id: "acc_personal".into(),
                    pot_id: "pot_personal".into(),
                },
                Destination {
                    allocation: "joint".into(),
                    account_id: "acc_joint".into(),
                    pot_id: "pot_joint".into(),
                },
            ],
        )
        .unwrap();
    assert_eq!(store.run(run).unwrap().operations.len(), 2);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM run_operations WHERE purchase_id=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn cutoff_survives_restart_and_applies_to_overlaps_and_new_old_purchases() {
    let (dir, mut store) = setup();
    store.set_cutover(DATE, "Historical boundary").unwrap();
    drop(store);
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.cutover_date().unwrap().as_deref(), Some(DATE));
    let overlap = csv(&[
        ("old", "2026-09-26"),
        ("boundary", DATE),
        ("another-old", "2026-09-26"),
        ("", "2026-09-26"),
        ("", DATE),
        ("", "null"),
    ]);
    let id = store.import("overlap.csv", &overlap).unwrap();
    assert_eq!(store.queue_summary().unwrap().outstanding, 2);
    assert_eq!(
        store.unresolved_exception_count().unwrap(),
        2,
        "boundary and undated exceptions remain"
    );
    assert_eq!(store.detail_row(4).unwrap().handling, "excluded");
    assert!(store.detail_row(2).unwrap().staged);
    assert_eq!(store.import("repeat.csv", &overlap).unwrap(), id);
    let revision = store.review_revision().unwrap();
    let repeated = store.set_cutover(DATE, "Same boundary").unwrap();
    assert_eq!((repeated.excluded, repeated.resolved_exceptions), (0, 0));
    assert_eq!(store.review_revision().unwrap(), revision);
    assert!(store.set_cutover("2026-10-01", "Changed boundary").is_err());
}

#[test]
fn cutover_rolls_back_exclusions_and_boundary_on_exception_failure() {
    let (dir, mut store) = setup();
    let before = store.detail_rows().unwrap();
    let revision = store.review_revision().unwrap();
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_cutover BEFORE INSERT ON exception_decisions BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(store.set_cutover(DATE, "Historical boundary").is_err());
    assert_eq!(store.cutover_date().unwrap(), None);
    assert_eq!(store.review_revision().unwrap(), revision);
    for prior in before {
        let after = store.detail_row(prior.id).unwrap();
        assert_eq!(prior.signature(), after.signature());
        assert!(after.staged);
    }
    for invalid in ["2026-02-30", "2026-9-27", ""] {
        assert!(store.set_cutover(invalid, "Historical boundary").is_err());
    }
}

#[test]
fn old_conflicting_observation_cannot_hide_a_current_purchase_conflict() {
    let (_dir, mut store) = setup();
    store.set_cutover(DATE, "Historical boundary").unwrap();
    store
        .import("conflict.csv", &csv(&[("boundary", "2026-09-26")]))
        .unwrap();
    assert!(store.detail_row(2).unwrap().conflict);
    assert_eq!(store.unresolved_exception_count().unwrap(), 1);
    assert!(!store.queue_summary().unwrap().runnable());
}

#[test]
fn active_run_blocks_cutover() {
    let (_dir, mut store) = setup();
    apply(
        &mut store,
        Change::Exception {
            observation_id: 4,
            reason: "Synthetic source evidence".into(),
        },
    );
    store
        .prepare_run(
            store.review_revision().unwrap(),
            &[
                Destination {
                    allocation: "personal".into(),
                    account_id: "acc_personal".into(),
                    pot_id: "pot_personal".into(),
                },
                Destination {
                    allocation: "joint".into(),
                    account_id: "acc_joint".into(),
                    pot_id: "pot_joint".into(),
                },
            ],
        )
        .unwrap();
    assert!(store.set_cutover(DATE, "Historical boundary").is_err());
    assert_eq!(store.cutover_date().unwrap(), None);
}
