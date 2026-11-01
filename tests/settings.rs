use recon::{
    Error, execution::Destination, queue::QueueSummary, review::Change,
    settings::DestinationSettings, storage::Store,
};
use rusqlite::Connection;
use std::collections::BTreeSet;

fn settings() -> DestinationSettings {
    DestinationSettings {
        version: 0,
        personal_account: "acc_personal".into(),
        personal_pot: "pot_personal".into(),
        joint_account: "acc_joint".into(),
        joint_pot: "pot_joint".into(),
    }
}
fn apply(s: &mut Store, change: Change) {
    let token = s.stage_review(change).unwrap();
    s.commit_review(&token, &BTreeSet::new(), true).unwrap();
}
fn ready(s: &mut Store) {
    s.import("synthetic.csv", include_bytes!("fixtures/virgin-first.csv"))
        .unwrap();
    apply(
        s,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Synthetic purchase".into(),
        },
    );
    apply(
        s,
        Change::AccountRule {
            merchant: "SYNTHETIC SHOP".into(),
            card: String::new(),
            account: "personal".into(),
        },
    );
    let rows = s
        .detail_rows()
        .unwrap()
        .iter()
        .map(|r| (r.id, r.signature()))
        .collect();
    apply(s, Change::Staging { rows, staged: true });
    apply(
        s,
        Change::Exception {
            observation_id: 3,
            reason: "Synthetic non-funding evidence".into(),
        },
    );
}

#[test]
fn destination_settings_survive_restart_and_backup_and_validate_as_a_pair() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path()).unwrap();
    assert_eq!(
        s.destination_settings().unwrap(),
        DestinationSettings::default()
    );
    let mut input = settings();
    input.personal_account = " acc_personal ".into();
    s.save_destination_settings(input).unwrap();
    let saved = s.destination_settings().unwrap();
    assert_eq!(saved.version, 1);
    assert_eq!(saved.personal_account, "acc_personal");
    let revision = s.review_revision().unwrap();
    s.save_destination_settings(saved.clone()).unwrap();
    assert_eq!(s.review_revision().unwrap(), revision);
    assert!(matches!(
        s.save_destination_settings(settings()),
        Err(Error::Stale)
    ));
    let backup = s.backup().unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    assert_eq!(s.destination_settings().unwrap(), saved);
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(backup, restored.path().join("recon.sqlite3")).unwrap();
    assert_eq!(
        Store::open(restored.path())
            .unwrap()
            .destination_settings()
            .unwrap(),
        saved
    );
}

#[test]
fn invalid_or_failed_config_saves_are_atomic_and_unused_allocations_can_be_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path()).unwrap();
    s.save_destination_settings(settings()).unwrap();
    let saved = s.destination_settings().unwrap();
    for invalid in ["", "pot_wrong", "acc_with space", "<script>bad</script>"] {
        let mut next = saved.clone();
        next.personal_account = invalid.into();
        assert!(s.save_destination_settings(next).is_err());
        assert_eq!(s.destination_settings().unwrap(), saved);
    }
    let mut shared = saved.clone();
    shared.joint_account = shared.personal_account.clone();
    assert!(s.save_destination_settings(shared).is_err());
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_settings BEFORE INSERT ON destination_settings BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let mut next = saved.clone();
    next.personal_pot = "pot_new".into();
    assert!(s.save_destination_settings(next).is_err());
    assert_eq!(s.destination_settings().unwrap(), saved);
    db.execute_batch("DROP TRIGGER fail_settings;").unwrap();
    let mut single = saved;
    single.joint_account.clear();
    single.joint_pot.clear();
    s.save_destination_settings(single).unwrap();
    let summary = QueueSummary {
        personal_count: 1,
        personal_pence: 123,
        ..Default::default()
    };
    let saved = s.destination_settings().unwrap();
    assert_eq!(saved.targets(&summary).unwrap()[0].1, 123);
    assert!(
        saved
            .targets(&QueueSummary {
                joint_count: 1,
                ..Default::default()
            })
            .is_err()
    );
}

#[test]
fn changing_destinations_invalidates_run_preview_but_preserves_staging_and_existing_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path()).unwrap();
    ready(&mut s);
    s.save_destination_settings(settings()).unwrap();
    let before = s.detail_rows().unwrap();
    let revision = s.review_revision().unwrap();
    let old_targets: Vec<Destination> = s
        .destination_settings()
        .unwrap()
        .targets(&s.queue_summary().unwrap())
        .unwrap()
        .into_iter()
        .map(|t| t.0)
        .collect();
    let mut next = s.destination_settings().unwrap();
    next.personal_pot = "pot_next".into();
    s.save_destination_settings(next).unwrap();
    assert!(matches!(
        s.prepare_run(revision, &old_targets),
        Err(Error::Stale)
    ));
    for row in before {
        let current = s.detail_row(row.id).unwrap();
        assert_eq!(row.signature(), current.signature());
        assert!(current.staged);
    }
    let targets: Vec<Destination> = s
        .destination_settings()
        .unwrap()
        .targets(&s.queue_summary().unwrap())
        .unwrap()
        .into_iter()
        .map(|t| t.0)
        .collect();
    let run = s
        .prepare_run(s.review_revision().unwrap(), &targets)
        .unwrap();
    let mut future = s.destination_settings().unwrap();
    future.personal_account = "acc_future".into();
    future.personal_pot = "pot_future".into();
    s.save_destination_settings(future).unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    assert!(
        s.run(run)
            .unwrap()
            .operations
            .iter()
            .all(|o| o.destination.account_id == "acc_personal"
                && o.destination.pot_id == "pot_next")
    );
    assert_eq!(
        s.destination_settings().unwrap().personal_account,
        "acc_future"
    );
}
