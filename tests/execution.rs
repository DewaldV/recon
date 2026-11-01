use recon::{
    execution::{self, Bank, BankFailure, Destination, Operation},
    review::Change,
    storage::Store,
};
use rusqlite::Connection;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct MockBank {
    attempts: Mutex<Vec<(i64, String)>>,
    notes: Mutex<Vec<(i64, String)>>,
    outcomes: Mutex<VecDeque<std::result::Result<(), BankFailure>>>,
    note_outcomes: Mutex<VecDeque<std::result::Result<(), BankFailure>>>,
    lookup_outcomes: Mutex<VecDeque<std::result::Result<(), BankFailure>>>,
    totals: Mutex<Vec<i64>>,
    reject_preflight: bool,
    same_transaction: bool,
    returned_transaction: bool,
    events: Mutex<Vec<(&'static str, i64)>>,
    lookups: Mutex<usize>,
}
impl Bank for MockBank {
    async fn preflight(
        &self,
        destinations: &[(Destination, i64)],
    ) -> std::result::Result<(), BankFailure> {
        self.totals
            .lock()
            .unwrap()
            .push(destinations.iter().map(|d| d.1).sum());
        if self.reject_preflight {
            Err(BankFailure::Unauthorized)
        } else {
            Ok(())
        }
    }
    async fn deposit(&self, op: &Operation) -> std::result::Result<Option<String>, BankFailure> {
        self.events.lock().unwrap().push(("deposit", op.id));
        self.attempts
            .lock()
            .unwrap()
            .push((op.id, op.dedupe_id.clone()));
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))?;
        Ok(self
            .returned_transaction
            .then(|| format!("tx_synthetic{}", op.id)))
    }
    async fn find_transactions(
        &self,
        operations: &[Operation],
        _used: &BTreeSet<String>,
    ) -> std::result::Result<Vec<(i64, String)>, BankFailure> {
        *self.lookups.lock().unwrap() += 1;
        self.lookup_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))?;
        Ok(operations
            .iter()
            .map(|op| {
                (
                    op.id,
                    if self.same_transaction {
                        "tx_same".into()
                    } else {
                        format!("tx_synthetic{}", op.id)
                    },
                )
            })
            .collect())
    }
    async fn annotate(&self, op: &Operation, id: &str) -> std::result::Result<(), BankFailure> {
        self.events.lock().unwrap().push(("note", op.id));
        self.notes.lock().unwrap().push((op.id, id.into()));
        self.note_outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }
}
fn apply(s: &mut Store, c: Change) {
    let t = s.stage_review(c).unwrap();
    s.commit_review(&t, &BTreeSet::new(), true).unwrap();
}
fn setup() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(d.path()).unwrap();
    s.import("test", include_bytes!("fixtures/virgin-first.csv"))
        .unwrap();
    apply(
        &mut s,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Synthetic note".into(),
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
    let c = Change::Staging {
        rows: s
            .detail_rows()
            .unwrap()
            .iter()
            .map(|r| (r.id, r.signature()))
            .collect(),
        staged: true,
    };
    apply(&mut s, c);
    apply(
        &mut s,
        Change::Exception {
            observation_id: 3,
            reason: "Synthetic row explicitly not outstanding".into(),
        },
    );
    (d, s)
}
fn targets() -> Vec<Destination> {
    vec![Destination {
        allocation: "personal".into(),
        account_id: "acc_synthetic".into(),
        pot_id: "pot_synthetic".into(),
    }]
}

#[tokio::test]
async fn one_run_spans_both_accounts_and_historical_import_preserves_instructions() {
    let (_dir, mut store) = setup();
    store.decide(1, 0, "joint").unwrap();
    let row = store
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    apply(
        &mut store,
        Change::Staging {
            rows: vec![(row.id, row.signature())],
            staged: true,
        },
    );
    let mut destinations = targets();
    destinations.push(Destination {
        allocation: "joint".into(),
        account_id: "acc_joint".into(),
        pot_id: "pot_joint".into(),
    });
    let id = store
        .prepare_run(store.review_revision().unwrap(), &destinations)
        .unwrap();
    let before = store.run(id).unwrap();
    assert_eq!(before.operations.len(), 2);
    assert!(before.operations.iter().any(|o|o.destination.allocation=="joint" && o.destination.account_id=="acc_joint"));
    assert!(
        before
            .operations
            .iter()
            .any(|o| o.destination.allocation == "personal"
                && o.destination.account_id == "acc_synthetic")
    );
    let change=store.parse_history("history.csv".into(),b"Reference,Account,Category,Description,Paid,Reason\n00000000000000000000001,Personal,Other,Must not change,Yes,Historical evidence\n".to_vec()).unwrap();
    apply(&mut store, change);
    let row = store
        .detail_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.id == 1)
        .unwrap();
    assert_eq!(
        (&*row.account, &*row.description, &*row.handling),
        ("joint", "Synthetic note", "outstanding")
    );
    let store = Arc::new(Mutex::new(store));
    let bank = MockBank::default();
    execution::execute(store.clone(), &bank, id).await.unwrap();
    assert_eq!(bank.attempts.lock().unwrap().len(), 2);
    let after = store.lock().unwrap().run(id).unwrap();
    for (old, new) in before.operations.iter().zip(&after.operations) {
        assert_eq!(
            (&old.dedupe_id, &old.note, old.amount_pence),
            (&new.dedupe_id, &new.note, new.amount_pence)
        );
    }
}

#[tokio::test]
async fn partial_failure_resume_and_notes_never_repeat_confirmed_deposits() {
    let (d, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    assert!(
        s.prepare_run(s.review_revision().unwrap(), &targets())
            .is_err()
    );
    let bank = MockBank::default();
    bank.outcomes
        .lock()
        .unwrap()
        .extend([Ok(()), Err(BankFailure::Uncertain)]);
    let s = Arc::new(Mutex::new(s));
    assert!(execution::execute(s.clone(), &bank, id).await.is_err());
    let run = s.lock().unwrap().run(id).unwrap();
    assert_eq!(run.operations[0].deposit, "confirmed");
    assert_eq!(run.operations[1].deposit, "unknown");
    assert_eq!(s.lock().unwrap().queue_summary().unwrap().outstanding, 1);
    drop(s);
    let s = Arc::new(Mutex::new(Store::open(d.path()).unwrap()));
    execution::execute(s.clone(), &bank, id).await.unwrap();
    let attempts = bank.attempts.lock().unwrap().clone();
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[1], attempts[2]);
    assert_eq!(*bank.totals.lock().unwrap(), vec![2468, 0]);
    let run = s.lock().unwrap().run(id).unwrap();
    assert!(
        run.operations
            .iter()
            .all(|o| o.deposit == "confirmed" && o.note_status == "complete")
    );
    assert_eq!(s.lock().unwrap().queue_summary().unwrap().outstanding, 0);
    assert_eq!(bank.notes.lock().unwrap().len(), 2);
    assert_eq!(s.lock().unwrap().run(id).unwrap().status, "complete");
    assert_eq!(bank.attempts.lock().unwrap().len(), 3);
    let db = Connection::open(d.path().join("recon.sqlite3")).unwrap();
    assert!(
        db.execute("UPDATE run_operations SET amount_pence=1", [])
            .is_err()
    );
}

#[tokio::test]
async fn preflight_expiry_sends_no_deposits_and_new_imports_do_not_enter_active_run() {
    let (_d, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    let s = Arc::new(Mutex::new(s));
    let bank = MockBank {
        reject_preflight: true,
        ..Default::default()
    };
    assert!(execution::execute(s.clone(), &bank, id).await.is_err());
    assert!(bank.attempts.lock().unwrap().is_empty());
    s.lock()
        .unwrap()
        .import("overlap", include_bytes!("fixtures/virgin-overlap.csv"))
        .unwrap();
    assert_eq!(s.lock().unwrap().run(id).unwrap().operations.len(), 2);
    assert_eq!(s.lock().unwrap().queue_summary().unwrap().needs_review, 1);
}

#[test]
fn final_run_snapshot_checks_versions_scope_and_destinations_atomically() {
    let (_d, mut s) = setup();
    let revision = s.review_revision().unwrap();
    s.decide(1, 0, "joint").unwrap();
    assert!(s.prepare_run(revision, &targets()).is_err());
    assert!(
        s.prepare_run(s.review_revision().unwrap(), &targets())
            .is_err()
    );
    assert!(s.active_run().unwrap().is_none());
    let c = Change::Staging {
        rows: s
            .detail_rows()
            .unwrap()
            .iter()
            .map(|r| (r.id, r.signature()))
            .collect(),
        staged: true,
    };
    apply(&mut s, c);
    assert!(
        s.prepare_run(s.review_revision().unwrap(), &targets())
            .is_err(),
        "joint destination missing"
    );
    assert!(s.run_ids().unwrap().is_empty());
}

#[tokio::test]
async fn note_mapping_is_unique_and_confirmed_purchase_cannot_be_reopened() {
    let (_d, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    let s = Arc::new(Mutex::new(s));
    let bank = MockBank {
        same_transaction: true,
        ..Default::default()
    };
    assert!(execution::execute(s.clone(), &bank, id).await.is_err());
    let ops = s.lock().unwrap().run(id).unwrap().operations;
    assert_eq!(ops[0].mapped_transaction, "tx_same");
    assert_eq!(ops[0].note_status, "complete");
    assert!(ops[1].mapped_transaction.is_empty());
    assert_eq!(ops[1].deposit, "confirmed");
    assert_eq!(bank.notes.lock().unwrap().len(), 1);
    assert!(
        s.lock()
            .unwrap()
            .stage_review(Change::Handling {
                ids: vec![1],
                state: "outstanding".into(),
                reason: "Should not fund again".into(),
                reopen: true
            })
            .is_err()
    );
}

#[tokio::test]
async fn crash_in_flight_reuses_the_persisted_key_and_backup_keeps_results() {
    let (d, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    let op = s.run(id).unwrap().operations.remove(0);
    let db = Connection::open(d.path().join("recon.sqlite3")).unwrap();
    db.execute("INSERT INTO operation_events(operation_id,deposit,note,message) VALUES(?1,'in_flight','pending','simulated crash')",[op.id]).unwrap();
    drop(db);
    drop(s);
    let s = Arc::new(Mutex::new(Store::open(d.path()).unwrap()));
    let bank = MockBank::default();
    execution::execute(s.clone(), &bank, id).await.unwrap();
    assert_eq!(bank.attempts.lock().unwrap()[0].1, op.dedupe_id);
    let snapshot = s.lock().unwrap().backup().unwrap();
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(snapshot, restored.path().join("recon.sqlite3")).unwrap();
    let s = Store::open(restored.path()).unwrap();
    assert!(
        s.run(id)
            .unwrap()
            .operations
            .iter()
            .all(|o| o.deposit == "confirmed")
    );
    assert_eq!(s.queue_summary().unwrap().outstanding, 0);
    assert!(
        s.verify_execution_guard().is_err(),
        "a database-only restore cannot execute"
    );
    std::fs::copy(
        d.path().join("execution.guard"),
        restored.path().join("execution.guard"),
    )
    .unwrap();
    s.verify_execution_guard().unwrap();
}

#[tokio::test]
async fn stale_restore_is_readable_but_cannot_send_money() {
    let (dir, mut store) = setup();
    let snapshot = store.backup().unwrap();
    let id = store
        .prepare_run(store.review_revision().unwrap(), &targets())
        .unwrap();
    let store = Arc::new(Mutex::new(store));
    let bank = MockBank::default();
    execution::execute(store, &bank, id).await.unwrap();
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(snapshot, restored.path().join("recon.sqlite3")).unwrap();
    std::fs::copy(
        dir.path().join("execution.guard"),
        restored.path().join("execution.guard"),
    )
    .unwrap();
    let mut store = Store::open(restored.path()).unwrap();
    assert_eq!(store.queue_summary().unwrap().staged, 2);
    assert!(
        store
            .prepare_run(store.review_revision().unwrap(), &targets())
            .is_err()
    );
    let bank = MockBank::default();
    assert!(
        execution::execute(Arc::new(Mutex::new(store)), &bank, id)
            .await
            .is_err()
    );
    assert!(bank.attempts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_note_retry_preserves_mapping_across_restart_without_depositing_again() {
    let (dir, mut store) = setup();
    let id = store
        .prepare_run(store.review_revision().unwrap(), &targets())
        .unwrap();
    let store = Arc::new(Mutex::new(store));
    let bank = MockBank::default();
    let op = store.lock().unwrap().run(id).unwrap().operations.remove(0);
    bank.note_outcomes
        .lock()
        .unwrap()
        .push_back(Err(BankFailure::Uncertain));
    assert!(execution::execute(store.clone(), &bank, id).await.is_err());
    drop(store);
    let store = Arc::new(Mutex::new(Store::open(dir.path()).unwrap()));
    let saved = store.lock().unwrap().run(id).unwrap().operations.remove(0);
    assert_eq!(
        (
            &*saved.deposit,
            &*saved.note_status,
            &*saved.mapped_transaction
        ),
        (
            "confirmed",
            "pending",
            format!("tx_synthetic{}", op.id).as_str()
        )
    );
    execution::execute(store.clone(), &bank, id).await.unwrap();
    assert_eq!(bank.attempts.lock().unwrap().len(), 2);
    assert_eq!(
        *bank.notes.lock().unwrap(),
        vec![
            (op.id, format!("tx_synthetic{}", op.id)),
            (op.id, format!("tx_synthetic{}", op.id)),
            (op.id + 1, format!("tx_synthetic{}", op.id + 1))
        ]
    );
    assert_eq!(
        *bank.lookups.lock().unwrap(),
        2,
        "a recorded mapping is not rediscovered on retry"
    );
    assert_eq!(store.lock().unwrap().run(id).unwrap().status, "complete");
    let history = store
        .lock()
        .unwrap()
        .purchase_history(op.purchase_id)
        .unwrap();
    for kind in [
        "staging",
        "execution",
        "note mapping",
        "related account rule",
        "related merchant default",
    ] {
        assert!(history.iter().any(|e| e.kind == kind), "missing {kind}");
    }
}

#[tokio::test]
async fn returned_transaction_is_persisted_and_note_precedes_next_deposit() {
    let (_dir, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    let ops = s.run(id).unwrap().operations;
    let s = Arc::new(Mutex::new(s));
    let bank = MockBank {
        returned_transaction: true,
        ..Default::default()
    };
    execution::execute(s.clone(), &bank, id).await.unwrap();
    assert_eq!(*bank.lookups.lock().unwrap(), 0);
    assert_eq!(
        *bank.events.lock().unwrap(),
        vec![
            ("deposit", ops[0].id),
            ("note", ops[0].id),
            ("deposit", ops[1].id),
            ("note", ops[1].id)
        ]
    );
    assert_eq!(s.lock().unwrap().run(id).unwrap().status, "complete");
}

#[tokio::test]
async fn legacy_confirmed_deposits_resume_all_notes_without_funds_or_deposit_calls() {
    let (dir, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    let ops = s.run(id).unwrap().operations;
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    for op in &ops {
        db.execute("INSERT INTO operation_events(operation_id,deposit,note,message) VALUES(?1,'in_flight','pending','Legacy submission')",[op.id]).unwrap();
        db.execute("INSERT INTO operation_events(operation_id,deposit,note,message) VALUES(?1,'confirmed','pending','Legacy deposits-only result')",[op.id]).unwrap();
    }
    let s = Arc::new(Mutex::new(s));
    let bank = MockBank {
        reject_preflight: true,
        ..Default::default()
    };
    bank.note_outcomes
        .lock()
        .unwrap()
        .push_back(Err(BankFailure::Unauthorized));
    assert!(execution::execute(s.clone(), &bank, id).await.is_err());
    assert!(
        s.lock()
            .unwrap()
            .run(id)
            .unwrap()
            .operations
            .iter()
            .all(|o| !o.mapped_transaction.is_empty() && o.note_status == "pending")
    );
    drop(s);
    let s = Arc::new(Mutex::new(Store::open(dir.path()).unwrap()));
    execution::execute(s.clone(), &bank, id).await.unwrap();
    assert!(bank.attempts.lock().unwrap().is_empty());
    assert!(bank.totals.lock().unwrap().is_empty());
    assert_eq!(bank.notes.lock().unwrap().len(), 3);
    assert_eq!(*bank.lookups.lock().unwrap(), 1);
    assert_eq!(s.lock().unwrap().run(id).unwrap().status, "complete");
    execution::execute(s.clone(), &bank, id).await.unwrap();
    assert_eq!(bank.notes.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn delayed_note_discovery_resumes_without_repeating_the_confirmed_deposit() {
    let (dir, mut s) = setup();
    let id = s
        .prepare_run(s.review_revision().unwrap(), &targets())
        .unwrap();
    let s = Arc::new(Mutex::new(s));
    let bank = MockBank::default();
    bank.lookup_outcomes
        .lock()
        .unwrap()
        .push_back(Err(BankFailure::NoteMatchPending));
    assert!(execution::execute(s.clone(), &bank, id).await.is_err());
    let run = s.lock().unwrap().run(id).unwrap();
    assert_eq!(run.status, "active");
    assert_eq!(run.operations[0].deposit, "confirmed");
    assert!(run.operations[0].mapped_transaction.is_empty());
    assert_eq!(run.operations[1].deposit, "pending");
    assert!(bank.notes.lock().unwrap().is_empty());
    assert_eq!(bank.attempts.lock().unwrap().len(), 1);
    drop(s);
    let s = Arc::new(Mutex::new(Store::open(dir.path()).unwrap()));
    execution::execute(s.clone(), &bank, id).await.unwrap();
    assert_eq!(bank.attempts.lock().unwrap().len(), 2);
    assert_eq!(
        *bank.events.lock().unwrap(),
        vec![
            ("deposit", run.operations[0].id),
            ("note", run.operations[0].id),
            ("deposit", run.operations[1].id),
            ("note", run.operations[1].id)
        ]
    );
    assert_eq!(s.lock().unwrap().run(id).unwrap().status, "complete");
}
