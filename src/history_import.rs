//! Read-only legacy CSV adapter. References must be explicitly verified before upload.
use crate::{
    Error, Result,
    importer::MAX_UPLOAD,
    review::{Change, DetailRow},
    storage::Store,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoricalRow {
    pub record: i64,
    pub purchase_id: i64,
    pub signature: String,
    pub account: String,
    pub category: String,
    pub description: String,
    pub external: bool,
    pub reason: String,
}
impl Store {
    pub fn parse_history(&self, filename: String, bytes: Vec<u8>) -> Result<Change> {
        if bytes.len() > MAX_UPLOAD || filename.len() > 255 {
            return Err(Error::Invalid(
                "Historical CSV exceeds upload limits.".into(),
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Invalid("Historical CSV must be UTF-8.".into()))?
            .trim_start_matches('\u{feff}');
        let mut csv = csv::Reader::from_reader(text.as_bytes());
        let headers = csv
            .headers()
            .map_err(|_| Error::Invalid("Invalid historical CSV header.".into()))?
            .clone();
        let required = [
            "Reference",
            "Account",
            "Category",
            "Description",
            "Paid",
            "Reason",
        ];
        if headers.len() != required.len()
            || required
                .iter()
                .any(|h| headers.iter().filter(|x| x == h).count() != 1)
        {
            return Err(Error::Invalid("Expected Reference,Account,Category,Description,Paid,Reason columns. Verify source references explicitly; fingerprints are not identities.".into()));
        }
        let indexes: Vec<_> = required
            .iter()
            .map(|h| headers.iter().position(|x| x == *h).unwrap())
            .collect();
        let current = self.detail_rows()?;
        let mut selected = BTreeSet::new();
        let mut rows = Vec::new();
        for (i, record) in csv.records().enumerate() {
            let record =
                record.map_err(|_| Error::Invalid("Invalid historical CSV row.".into()))?;
            let values: Vec<_> = indexes.iter().map(|i| record[*i].trim()).collect();
            let account = values[1].to_lowercase();
            if !matches!(account.as_str(), "" | "personal" | "joint")
                || !matches!(values[4], "Yes" | "No" | "")
                || values[2].len() > 200
                || values[3].len() > 4000
                || values[5].is_empty()
                || values[5].len() > 3500
            {
                return Err(Error::Invalid(format!(
                    "Check account, Paid (Yes/No), details, and evidence at historical record {}.",
                    i + 2
                )));
            }
            let id:Option<i64>=self.db.query_row("SELECT p.id FROM purchases p JOIN source_accounts a ON a.id=p.source_account_id WHERE a.account_key='virgin-primary' AND p.reference=?1",[values[0]],|r|r.get(0)).optional()?;
            let id=id.ok_or_else(||Error::Invalid(format!("Historical record {} has no matching verified source reference. Resolve the mapping before importing.",i+2)))?;
            if !selected.insert(id) {
                return Err(Error::Invalid(
                    "Historical CSV selects a purchase more than once.".into(),
                ));
            }
            let row = current.iter().find(|r| r.id == id).ok_or(Error::NotFound)?;
            if row.conflict {
                return Err(Error::Invalid(
                    "Resolve source conflicts before migrating historical decisions.".into(),
                ));
            }
            rows.push(HistoricalRow {
                record: i as i64 + 2,
                purchase_id: id,
                signature: row.signature(),
                account,
                category: values[2].into(),
                description: values[3].into(),
                external: values[4] == "Yes",
                reason: values[5].into(),
            });
            if rows.len() > 100 {
                return Err(Error::Invalid(
                    "Historical migration is limited to 100 explicitly matched purchases per CSV."
                        .into(),
                ));
            }
        }
        if rows.is_empty() {
            return Err(Error::Invalid(
                "Historical CSV contains no decisions.".into(),
            ));
        }
        Ok(Change::Historical {
            filename,
            bytes,
            rows,
        })
    }
    pub(crate) fn validate_history(&self, rows: &[HistoricalRow]) -> Result<()> {
        let current = self.detail_rows()?;
        for imported in rows {
            let row = current
                .iter()
                .find(|r| r.id == imported.purchase_id)
                .ok_or(Error::NotFound)?;
            if row.signature() != imported.signature {
                return Err(Error::Stale);
            }
        }
        Ok(())
    }
}

pub(crate) fn save_history(
    tx: &rusqlite::Transaction<'_>,
    filename: &str,
    bytes: &[u8],
    rows: &[HistoricalRow],
    current: &[DetailRow],
    allow_categories: bool,
) -> Result<()> {
    tx.execute(
        "INSERT INTO historical_sources(sha256,filename,bytes) VALUES(?1,?2,?3)",
        params![format!("{:x}", Sha256::digest(bytes)), filename, bytes],
    )?;
    let source = tx.last_insert_rowid();
    for imported in rows {
        let row = current
            .iter()
            .find(|r| r.id == imported.purchase_id)
            .ok_or(Error::NotFound)?;
        tx.execute("INSERT INTO historical_matches(source_id,record,purchase_id,proposed,previous_snapshot) VALUES(?1,?2,?3,?4,?5)",params![source,imported.record,row.id,serde_json::to_string(imported).map_err(|_|Error::Worker)?,row.snapshot()])?;
        if row.in_run {
            continue;
        }
        if row.version == 0 && !imported.account.is_empty() {
            tx.execute(
                "INSERT INTO review_decisions(purchase_id,version,account) VALUES(?1,1,?2)",
                params![row.id, imported.account],
            )?;
        }
        let (category, description): (Option<String>, Option<String>) = tx
            .query_row(
                "SELECT category,description FROM current_details WHERE purchase_id=?1",
                [row.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or_default();
        let new_category = if row.details_version > 0 {
            category.clone()
        } else {
            crate::review::category_name(tx, &imported.category, allow_categories)?
        };
        let new_description = if row.details_version > 0 {
            description.clone()
        } else {
            (!imported.description.is_empty()).then(|| imported.description.clone())
        };
        if category != new_category || description != new_description {
            tx.execute("INSERT INTO purchase_details(purchase_id,version,category,description) VALUES(?1,?2,?3,?4)",params![row.id,row.details_version+1,new_category,new_description])?;
        }
        if row.handling_version == 0 && imported.external {
            tx.execute("INSERT INTO handling_decisions(purchase_id,version,state,reason,reopen_confirmed,provenance) VALUES(?1,1,'external',?2,0,'manual')",params![row.id,format!("Historical source #{source}, record {}: {}",imported.record,imported.reason)])?;
        }
    }
    Ok(())
}
