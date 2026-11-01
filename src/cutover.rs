//! Fixed posting-date boundary between historical spreadsheets and this ledger.
use crate::{Error, Result, importer, storage::Store};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;

pub struct CutoverResult {
    pub backup: PathBuf,
    pub excluded: usize,
    pub resolved_exceptions: usize,
}

impl Store {
    pub fn cutover_date(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT posting_date FROM recon_cutover WHERE id=1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Preserve every source and decision; append exclusions in one transaction.
    pub fn set_cutover(&mut self, date: &str, reason: &str) -> Result<CutoverResult> {
        if !importer::date(date) || reason.trim().is_empty() || reason.len() > 3500 {
            return Err(Error::Invalid(
                "Provide a YYYY-MM-DD start date and historical evidence.".into(),
            ));
        }
        if self.cutover_date()?.is_some_and(|current| current != date) {
            return Err(Error::Invalid(
                "The permanent recon start date is already configured.".into(),
            ));
        }
        if self.active_run()?.is_some() || self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM run_operations o JOIN purchases p ON p.id=o.purchase_id WHERE p.posting_date<?1)",
            [date], |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Invalid("Cannot set a cutover during an active run or across recorded run instructions.".into()));
        }
        let backup = self.backup()?;
        self.atomic(|store| {
            store.db.execute(
                "INSERT OR IGNORE INTO recon_cutover(id,posting_date,reason) VALUES(1,?1,?2)",
                params![date, reason],
            )?;
            let excluded = exclude_purchases(&store.db)?;
            let imports: Vec<(i64, Vec<u8>)> = store
                .db
                .prepare("SELECT id,original_bytes FROM imports ORDER BY id")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut resolved_exceptions = 0;
            for (id, bytes) in imports {
                for observation in importer::parse(&bytes)? {
                    resolved_exceptions += resolve_observation(&store.db, id, &observation, date)?;
                }
            }
            Ok(CutoverResult {
                backup,
                excluded,
                resolved_exceptions,
            })
        })
    }
}

pub(crate) fn exclude_purchases(db: &Connection) -> Result<usize> {
    Ok(db.execute(
        "INSERT INTO handling_decisions(purchase_id,version,state,reason,reopen_confirmed,provenance)
         SELECT p.id,coalesce(h.version,0)+1,'excluded','Before recon start date ' || c.posting_date || ': ' || c.reason,0,'manual'
         FROM purchases p JOIN recon_cutover c ON p.posting_date<c.posting_date
         LEFT JOIN current_handling h ON h.purchase_id=p.id
         WHERE coalesce(h.state,'unknown') IN ('unknown','outstanding')", [],
    )?)
}

pub(crate) fn resolve_observation(
    db: &Connection,
    import: i64,
    observation: &importer::Observation,
    date: &str,
) -> Result<usize> {
    if observation
        .posting_date
        .as_deref()
        .is_none_or(|posting| posting >= date)
    {
        return Ok(0);
    }
    // A conflicting old observation must never hide a current purchase's conflict.
    Ok(db.execute(
        "INSERT OR IGNORE INTO exception_decisions(observation_id,reason)
         SELECT o.id,'Before recon start date ' || c.posting_date || ': ' || c.reason
         FROM source_observations o CROSS JOIN recon_cutover c
         WHERE o.import_id=?1 AND o.record_number=?2
         AND o.disposition NOT IN ('purchase','pending','payment')
         AND (o.purchase_id IS NULL OR EXISTS(SELECT 1 FROM purchases p WHERE p.id=o.purchase_id AND p.posting_date<c.posting_date))",
        params![import,observation.record_number],
    )?)
}
