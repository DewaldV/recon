//! Recover previously quarantined rows through the same parser as new imports.
use crate::{Error, Result, importer, storage::Store};
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::BTreeMap, path::PathBuf};

pub struct RepairResult {
    pub repaired: usize,
    pub backup: Option<PathBuf>,
}

impl Store {
    /// Used on identical-file reuploads and explicitly for an existing import.
    /// Already dispositioned rows are never turned back into funding candidates.
    pub fn repair_import(&mut self, import_id: i64) -> Result<RepairResult> {
        let bytes: Vec<u8> = self
            .db
            .query_row(
                "SELECT original_bytes FROM imports WHERE id=?1",
                [import_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let unresolved: BTreeMap<i64,i64> = self.db.prepare(
            "SELECT record_number,id FROM source_observations o WHERE import_id=?1 AND disposition='malformed'
             AND NOT EXISTS(SELECT 1 FROM exception_decisions e WHERE e.observation_id=o.id)
             AND NOT EXISTS(SELECT 1 FROM observation_repairs r WHERE r.observation_id=o.id)")?
            .query_map([import_id], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        if unresolved.is_empty() {
            return Ok(RepairResult {
                repaired: 0,
                backup: None,
            });
        }
        let candidates: Vec<_> = importer::parse(&bytes)?
            .into_iter()
            .filter_map(|row| {
                let id = unresolved.get(&row.record_number)?;
                // Duplicate references are checked across the entire original file.
                if matches!(
                    row.disposition,
                    "malformed" | "unsupported" | "duplicate_reference"
                ) {
                    return None;
                }
                row.repaired.map(|fields| (*id, fields))
            })
            .collect();
        if candidates.is_empty() {
            return Ok(RepairResult {
                repaired: 0,
                backup: None,
            });
        }
        if self.active_run()?.is_some() {
            return Err(Error::Invalid(
                "Finish the active recon before recovering quarantined purchases.".into(),
            ));
        }
        let backup = self.backup()?;
        self.atomic(|s| {
            let records: Vec<_> = candidates.iter().map(|(_,fields)|fields.clone()).collect();
            let corrected = importer::repaired_csv(&records)?;
            let replacement = s.import_observations(&format!("repaired-import-{import_id}.csv"), &corrected, importer::parse(&corrected)?)?;
            for (index,(original,fields)) in candidates.iter().enumerate() {
                let record = index as i64 + 2;
                let replacement_observation: i64 = s.db.query_row("SELECT id FROM source_observations WHERE import_id=?1 AND record_number=?2", params![replacement,record], |r| r.get(0))?;
                record_repair(&s.db,*original,replacement_observation,fields)?;
                s.db.execute("INSERT INTO exception_decisions(observation_id,reason) VALUES(?1,?2)",params![original,format!("Auto-repaired using {}; replaced by import #{replacement}, record {record}. Replacement classification and the permanent posting-date cutoff apply.",importer::REPAIR_METHOD)])?;
            }
            Ok(RepairResult {repaired:candidates.len(),backup:Some(backup)})
        })
    }
}

pub(crate) fn record_repair(
    db: &Connection,
    original: i64,
    replacement: i64,
    fields: &[String],
) -> Result<()> {
    db.execute("INSERT INTO observation_repairs(observation_id,replacement_observation_id,method,normalized_record) VALUES(?1,?2,?3,?4)",params![original,replacement,importer::REPAIR_METHOD,serde_json::to_string(fields).map_err(|_|Error::Worker)?])?;
    Ok(())
}
