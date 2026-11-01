//! Persist instructions before sending requests. Recovery always reuses operation identity.
use crate::{Error, Result, queue::QueueSummary, storage::Store};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Destination {
    pub allocation: String,
    pub account_id: String,
    pub pot_id: String,
}
impl Destination {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.allocation.as_str(), "personal" | "joint")
            || !valid_id(&self.account_id, "acc_")
            || !valid_id(&self.pot_id, "pot_")
        {
            return Err(Error::Invalid(
                "Enter valid Monzo account and pot IDs for each allocation.".into(),
            ));
        }
        Ok(())
    }
}
pub fn valid_id(value: &str, prefix: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() < 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[derive(Clone, Debug)]
pub struct Operation {
    pub id: i64,
    pub run_id: i64,
    pub purchase_id: i64,
    pub destination: Destination,
    pub amount_pence: i64,
    pub note: String,
    pub merchant: String,
    pub dedupe_id: String,
    pub deposit: String,
    pub note_status: String,
    pub message: String,
    pub started_at: String,
    pub mapped_transaction: String,
}
impl Operation {
    pub fn amount(&self) -> String {
        crate::recon::format_pennies(self.amount_pence)
    }
}
pub struct Run {
    pub id: i64,
    pub status: String,
    pub created_at: String,
    pub operations: Vec<Operation>,
}

#[derive(Debug)]
pub enum BankFailure {
    Rejected,
    Unauthorized,
    RateLimited,
    Uncertain,
}
impl BankFailure {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Rejected => {
                "Monzo rejected this request. Check the destination and funds before resuming."
            }
            Self::Unauthorized => "Monzo access expired or was denied. Resume with a new token.",
            Self::RateLimited => "Monzo rate limit reached. Resume later.",
            Self::Uncertain => {
                "The remote outcome is unknown. Resume reuses the same deduplication key."
            }
        }
    }
}
pub trait Bank: Send + Sync {
    fn preflight(
        &self,
        destinations: &[(Destination, i64)],
    ) -> impl Future<Output = std::result::Result<(), BankFailure>> + Send;
    fn deposit(
        &self,
        operation: &Operation,
    ) -> impl Future<Output = std::result::Result<(), BankFailure>> + Send;
    fn validate_note(
        &self,
        _operation: &Operation,
        _transaction_id: &str,
    ) -> impl Future<Output = std::result::Result<(), BankFailure>> + Send {
        async { Ok(()) }
    }
    /// An explicit, user-confirmed transaction mapping is required: the deposit response has no transaction ID.
    fn annotate(
        &self,
        operation: &Operation,
        transaction_id: &str,
    ) -> impl Future<Output = std::result::Result<(), BankFailure>> + Send;
}

impl Store {
    /// A separate watermark detects a SQLite restore which predates submitted work.
    /// Missing guards on an existing schema are never silently recreated.
    pub fn verify_execution_guard(&self) -> Result<()> {
        let path = self.directory.join("execution.guard");
        let bytes=std::fs::read(&path).map_err(|_|Error::Invalid("Execution recovery record is missing. Restore it alongside the database and reconcile bank outcomes before execution.".into()))?;
        let (event, operation, key): (i64, i64, String) = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("Invalid execution recovery record.".into()))?;
        let latest: i64 = self.db.query_row(
            "SELECT coalesce(max(id),0) FROM operation_events",
            [],
            |r| r.get(0),
        )?;
        let matches = operation == 0
            || self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM run_operations WHERE id=?1 AND dedupe_id=?2)",
                params![operation, key],
                |r| r.get::<_, bool>(0),
            )?;
        if latest < event || !matches {
            return Err(Error::Invalid("This database predates recorded execution. Reconcile the restored ledger with bank outcomes before running or resuming.".into()));
        }
        Ok(())
    }
    pub(crate) fn write_execution_guard(&self) -> Result<()> {
        use std::io::Write;
        let event: i64 = self.db.query_row(
            "SELECT coalesce(max(id),0) FROM operation_events",
            [],
            |r| r.get(0),
        )?;
        let (operation, key): (i64, String) = self
            .db
            .query_row(
                "SELECT id,dedupe_id FROM run_operations ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or_default();
        let pending = self.directory.join("execution.guard.pending");
        let mut file = crate::config::private_file(&pending)?;
        file.set_len(0)?;
        file.write_all(&serde_json::to_vec(&(event, operation, key)).map_err(|_| Error::Worker)?)?;
        file.sync_all()?;
        std::fs::rename(pending, self.directory.join("execution.guard"))?;
        std::fs::File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
    pub fn review_revision(&self) -> Result<i64> {
        Ok(self
            .db
            .query_row("SELECT revision FROM review_revision WHERE id=1", [], |r| {
                r.get(0)
            })?)
    }
    pub fn active_run(&self) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row("SELECT id FROM recon_runs WHERE status='active'", [], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn run_ids(&self) -> Result<Vec<i64>> {
        Ok(self
            .db
            .prepare("SELECT id FROM recon_runs ORDER BY id DESC")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn run(&self, id: i64) -> Result<Run> {
        let (status, created_at) = self
            .db
            .query_row(
                "SELECT status,created_at FROM recon_runs WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let operations = self.db.prepare("SELECT id,run_id,purchase_id,allocation,account_id,pot_id,amount_pence,note,merchant,dedupe_id,deposit,note_status,message,
            (SELECT min(created_at) FROM operation_events e WHERE e.operation_id=current_operations.id),coalesce((SELECT transaction_id FROM note_mappings m WHERE m.operation_id=current_operations.id),'')
            FROM current_operations WHERE run_id=?1 ORDER BY id")?.query_map([id], |r| Ok(Operation {
                id:r.get(0)?,run_id:r.get(1)?,purchase_id:r.get(2)?,destination:Destination {allocation:r.get(3)?,account_id:r.get(4)?,pot_id:r.get(5)?},
                amount_pence:r.get(6)?,note:r.get(7)?,merchant:r.get(8)?,dedupe_id:r.get(9)?,deposit:r.get(10)?,note_status:r.get(11)?,message:r.get(12)?,
                started_at:r.get(13)?,mapped_transaction:r.get(14)?,
            }))?.collect::<rusqlite::Result<_>>()?;
        Ok(Run {
            id,
            status,
            created_at,
            operations,
        })
    }
    pub fn prepare_run(&mut self, revision: i64, destinations: &[Destination]) -> Result<i64> {
        self.verify_execution_guard()?;
        self.backup()?;
        // Serialize validation and snapshotting with any other SQLite writer.
        let id = self.atomic(|s| s.prepare_run_inner(revision, destinations))?;
        self.write_execution_guard()?;
        Ok(id)
    }
    fn prepare_run_inner(&self, revision: i64, destinations: &[Destination]) -> Result<i64> {
        if self.review_revision()? != revision {
            return Err(Error::Stale);
        }
        if self.active_run()?.is_some() {
            return Err(Error::Invalid(
                "Resume the current recon before starting another.".into(),
            ));
        }
        let rows = self.detail_rows()?;
        let summary = QueueSummary::from_rows(&rows, self.unresolved_exception_count()?)?;
        if !summary.runnable() {
            return Err(Error::Invalid("Stage every outstanding purchase and resolve source exceptions before running recon.".into()));
        }
        let mut targets = BTreeMap::new();
        for d in destinations {
            d.validate()?;
            if targets.insert(d.allocation.clone(), d).is_some() {
                return Err(Error::Invalid("Duplicate allocation destination.".into()));
            }
        }
        if targets.len() == 2 && targets["personal"].account_id == targets["joint"].account_id {
            return Err(Error::Invalid(
                "Personal and Joint require distinct source accounts.".into(),
            ));
        }
        self.db
            .execute("INSERT INTO recon_runs(status) VALUES('active')", [])?;
        let id = self.db.last_insert_rowid();
        for row in rows.iter().filter(|r| r.outstanding()) {
            let target = targets.get(&row.account).ok_or_else(|| {
                Error::Invalid("Configure the destination for each staged account.".into())
            })?;
            self.db.execute("INSERT INTO run_operations(run_id,purchase_id,allocation,account_id,pot_id,amount_pence,note,merchant,signature,snapshot,dedupe_id)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![id,row.id,row.account,target.account_id,target.pot_id,row.amount_pence,row.description,row.merchant,row.signature(),row.snapshot(),crate::config::random_secret()?])?;
            let op = self.db.last_insert_rowid();
            self.record_operation(op, "pending", "pending", "Ready to submit", None)?;
        }
        Ok(id)
    }
    pub(crate) fn record_operation(
        &self,
        id: i64,
        deposit: &str,
        note: &str,
        message: &str,
        transaction: Option<&str>,
    ) -> Result<()> {
        self.db.execute("INSERT INTO operation_events(operation_id,deposit,note,message,transaction_id) VALUES(?1,?2,?3,?4,?5)", params![id,deposit,note,message,transaction])?;
        Ok(())
    }
    pub(crate) fn enrich_execution(&self, rows: &mut [crate::review::DetailRow]) -> Result<()> {
        let states: BTreeMap<i64, (String, String)> = self
            .db
            .prepare("SELECT purchase_id,deposit,snapshot FROM current_operations")?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<rusqlite::Result<_>>()?;
        for row in rows {
            if let Some((state, snapshot)) = states.get(&row.id) {
                row.in_run = true;
                let fields: serde_json::Value =
                    serde_json::from_str(snapshot).map_err(|_| Error::Worker)?;
                for (field, key) in [
                    (&mut row.account, "account"),
                    (&mut row.account_source, "account_source"),
                    (&mut row.category, "category"),
                    (&mut row.category_source, "category_source"),
                    (&mut row.description, "description"),
                    (&mut row.description_source, "description_source"),
                ] {
                    *field = fields[key].as_str().ok_or(Error::Worker)?.into();
                }
                if state == "confirmed" {
                    row.handling = "funded".into();
                }
            }
        }
        Ok(())
    }
    fn finish_run(&self, id: i64) -> Result<()> {
        self.db.execute("UPDATE recon_runs SET status='complete' WHERE id=?1 AND status='active'
            AND NOT EXISTS(SELECT 1 FROM current_operations WHERE run_id=?1 AND (deposit!='confirmed' OR note_status!='complete'))",[id])?;
        Ok(())
    }
    pub fn execution_editable(&self, ids: &[i64]) -> Result<()> {
        for id in ids {
            if self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM run_operations WHERE purchase_id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::Invalid("This purchase is part of a recorded run. Its execution instructions are fixed.".into()));
            }
        }
        Ok(())
    }
}

pub async fn storage<T: Send + 'static>(
    store: Arc<Mutex<Store>>,
    f: impl FnOnce(&mut Store) -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        let mut guard = store.lock().map_err(|_| Error::Worker)?;
        f(&mut guard)
    })
    .await
    .map_err(|_| Error::Worker)?
}

/// The caller holds the single execution permit across this whole operation.
pub async fn execute<B: Bank>(store: Arc<Mutex<Store>>, bank: &B, id: i64) -> Result<()> {
    let run = storage(store.clone(), move |s| {
        s.verify_execution_guard()?;
        s.run(id)
    })
    .await?;
    let mut totals: BTreeMap<String, (Destination, i64)> = BTreeMap::new();
    for op in run.operations.iter().filter(|o| o.deposit != "confirmed") {
        let total = totals
            .entry(op.destination.allocation.clone())
            .or_insert((op.destination.clone(), 0));
        // An uncertain operation may already have debited the account. Its same-key
        // retry reconciles that outcome; don't require the already-moved funds again.
        if !matches!(op.deposit.as_str(), "unknown" | "in_flight") {
            total.1 = total
                .1
                .checked_add(op.amount_pence)
                .ok_or_else(|| Error::Invalid("Run total overflow.".into()))?;
        }
    }
    if let Err(error) = bank
        .preflight(&totals.into_values().collect::<Vec<_>>())
        .await
    {
        if let Some(op) = run.operations.iter().find(|o| o.deposit != "confirmed") {
            let op = op.clone();
            let message = error.message();
            storage(store.clone(), move |s| {
                s.record_operation(op.id, &op.deposit, "pending", message, None)?;
                s.write_execution_guard()
            })
            .await?;
        }
        return Err(Error::Invalid(error.message().into()));
    }
    for op in run.operations.iter().filter(|o| o.deposit != "confirmed") {
        let op_id = op.id;
        let purchase = op.purchase_id;
        storage(store.clone(),move|s| {
            if s.db.query_row("SELECT EXISTS(SELECT 1 FROM source_observations o WHERE purchase_id=?1 AND disposition='conflict' AND NOT EXISTS(SELECT 1 FROM exception_decisions e WHERE e.observation_id=o.id))",[purchase],|r|r.get::<_,bool>(0))? {
                return Err(Error::Invalid("A new source conflict blocks this run. Resolve the source evidence before continuing.".into()));
            }
            s.verify_execution_guard()?;
            s.record_operation(op_id,"in_flight","pending","Submitting with persisted deduplication key",None)?;
            s.write_execution_guard()
        }).await?;
        match bank.deposit(op).await {
            Ok(()) => {
                storage(store.clone(), move |s| {
                    s.record_operation(
                        op_id,
                        "confirmed",
                        "pending",
                        "Deposit confirmed; match its Monzo transaction to apply the note",
                        None,
                    )?;
                    s.write_execution_guard()
                })
                .await?
            }
            Err(error) => {
                let status = if matches!(error, BankFailure::Uncertain)
                    || matches!(op.deposit.as_str(), "unknown" | "in_flight")
                {
                    "unknown"
                } else {
                    "failed"
                };
                let message = error.message();
                storage(store.clone(), move |s| {
                    s.record_operation(op_id, status, "pending", message, None)?;
                    s.write_execution_guard()
                })
                .await?;
                return Err(Error::Invalid(message.into()));
            }
        }
    }
    storage(store, move |s| s.finish_run(id)).await
}

pub async fn apply_note<B: Bank>(
    store: Arc<Mutex<Store>>,
    bank: &B,
    run_id: i64,
    operation_id: i64,
    transaction: String,
) -> Result<()> {
    if !valid_id(&transaction, "tx_") {
        return Err(Error::Invalid(
            "Provide the confirmed Monzo transaction ID.".into(),
        ));
    }
    let id = transaction.clone();
    let operation=storage(store.clone(),move|s| {
        s.verify_execution_guard()?;
        let op=s.run(run_id)?.operations.into_iter().find(|o|o.id==operation_id).ok_or(Error::NotFound)?;
        if op.deposit!="confirmed" || op.note_status=="complete" {return Err(Error::Invalid("Only confirmed deposits with pending notes can be annotated.".into()));}
        let previous:Option<String>=s.db.query_row("SELECT transaction_id FROM note_mappings WHERE operation_id=?1",[operation_id],|r|r.get(0)).optional()?;
        if previous.as_ref().is_some_and(|previous|previous!=&id) {return Err(Error::Invalid("Retry the original transaction mapping; a note submission may already have succeeded.".into()));}
        if s.db.query_row("SELECT EXISTS(SELECT 1 FROM note_mappings WHERE transaction_id=?1 AND operation_id!=?2)",params![id,operation_id],|r|r.get::<_,bool>(0))? {return Err(Error::Invalid("That transaction is already assigned to a purchase.".into()));}
        Ok(op)
    }).await?;
    bank.validate_note(&operation, &transaction)
        .await
        .map_err(|e| Error::Invalid(e.message().into()))?;
    let id = transaction.clone();
    storage(store.clone(),move|s| {
        s.atomic(|s| {
            s.db.execute("INSERT INTO note_mappings(operation_id,transaction_id) VALUES(?1,?2) ON CONFLICT(operation_id) DO NOTHING",params![operation_id,id])?;
            let mapped:String=s.db.query_row("SELECT transaction_id FROM note_mappings WHERE operation_id=?1",[operation_id],|r|r.get(0))?;
            if mapped!=id {return Err(Error::Stale);}
            s.record_operation(operation_id,"confirmed","pending","Explicit transaction mapping reserved for note submission",None)
        })?;
        s.write_execution_guard()
    }).await?;
    if let Err(error) = bank.annotate(&operation, &transaction).await {
        let message = error.message();
        storage(store.clone(), move |s| {
            s.record_operation(operation_id, "confirmed", "pending", message, None)?;
            s.write_execution_guard()
        })
        .await?;
        return Err(Error::Invalid(message.into()));
    }
    storage(store, move |s| {
        s.atomic(|s| {
            s.record_operation(
                operation_id,
                "confirmed",
                "complete",
                "Note applied to explicitly confirmed transaction",
                Some(&transaction),
            )?;
            s.finish_run(run_id)
        })?;
        s.write_execution_guard()
    })
    .await
}
