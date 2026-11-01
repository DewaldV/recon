//! One global queue. Staging approves a particular effective decision snapshot.
use crate::{Error, Result, review::DetailRow, storage::Store};
use rusqlite::params;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eligibility {
    Handled,
    SourceConflict,
    MissingDetails,
    NeedsReview,
    Staged,
}

#[derive(Serialize)]
struct Snapshot<'a> {
    id: i64,
    amount_pence: i64,
    account: &'a str,
    account_version: i64,
    account_source: &'a str,
    category: &'a str,
    category_source: &'a str,
    description: &'a str,
    description_source: &'a str,
    details_version: i64,
    handling: &'a str,
    handling_version: i64,
    conflict: bool,
    source_version: i64,
}

impl DetailRow {
    pub fn snapshot(&self) -> String {
        serde_json::to_string(&Snapshot {
            id: self.id,
            amount_pence: self.amount_pence,
            account: &self.account,
            account_version: self.version,
            account_source: &self.account_source,
            category: &self.category,
            category_source: &self.category_source,
            description: &self.description,
            description_source: &self.description_source,
            details_version: self.details_version,
            handling: &self.handling,
            handling_version: self.handling_version,
            conflict: self.conflict,
            source_version: self.source_version,
        })
        .expect("snapshot contains only JSON primitives")
    }
    pub fn signature(&self) -> String {
        format!("{:x}", Sha256::digest(self.snapshot()))
    }
    pub fn outstanding(&self) -> bool {
        !matches!(self.handling.as_str(), "external" | "excluded" | "funded")
    }
    pub fn eligibility(&self) -> Eligibility {
        if !self.outstanding() {
            Eligibility::Handled
        } else if self.conflict {
            Eligibility::SourceConflict
        } else if !self.complete() || self.amount_pence <= 0 {
            Eligibility::MissingDetails
        } else if self.staged && self.handling == "outstanding" {
            Eligibility::Staged
        } else {
            Eligibility::NeedsReview
        }
    }
    pub fn queue_status(&self) -> &'static str {
        match self.eligibility() {
            Eligibility::Handled => match self.handling.as_str() {
                "external" => "Handled externally",
                "funded" => "Funded by recon",
                _ => "Excluded",
            },
            Eligibility::SourceConflict => "Source conflict",
            Eligibility::MissingDetails => "Complete missing details",
            Eligibility::NeedsReview => "Review and stage",
            Eligibility::Staged => "Staged",
        }
    }
    pub fn queue_status_class(&self) -> &'static str {
        match self.eligibility() {
            Eligibility::NeedsReview | Eligibility::Staged => "success",
            Eligibility::MissingDetails if self.amount_pence > 0 => "missing",
            _ => "error",
        }
    }
    pub fn can_stage(&self) -> bool {
        self.outstanding() && self.complete() && self.amount_pence > 0
    }
}

#[derive(Default)]
pub struct QueueSummary {
    pub outstanding: usize,
    pub needs_review: usize,
    pub staged: usize,
    pub personal_count: usize,
    pub joint_count: usize,
    pub personal_pence: i64,
    pub joint_pence: i64,
    pub exceptions: usize,
}
impl QueueSummary {
    pub fn personal_amount(&self) -> String {
        crate::recon::format_pennies(self.personal_pence)
    }
    pub fn joint_amount(&self) -> String {
        crate::recon::format_pennies(self.joint_pence)
    }
    pub fn runnable(&self) -> bool {
        self.staged > 0 && self.needs_review == 0 && self.exceptions == 0
    }
    pub fn from_rows(rows: &[DetailRow], exceptions: usize) -> Result<Self> {
        let mut s = Self {
            exceptions,
            ..Self::default()
        };
        for r in rows.iter().filter(|r| r.outstanding()) {
            s.outstanding += 1;
            if r.eligibility() != Eligibility::Staged {
                s.needs_review += 1;
                continue;
            }
            s.staged += 1;
            let total = match r.account.as_str() {
                "personal" => {
                    s.personal_count += 1;
                    &mut s.personal_pence
                }
                "joint" => {
                    s.joint_count += 1;
                    &mut s.joint_pence
                }
                _ => return Err(Error::Invalid("Invalid staged account.".into())),
            };
            *total = total.checked_add(r.amount_pence).ok_or_else(|| {
                Error::Invalid("Recon total exceeds the integer-penny range.".into())
            })?;
        }
        Ok(s)
    }
}

impl Store {
    pub(crate) fn enrich_staging(&self, rows: &mut [DetailRow]) -> Result<()> {
        let stages: BTreeMap<i64, String> = self
            .db
            .prepare("SELECT purchase_id,signature FROM current_staging WHERE staged=1")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for row in rows {
            row.staged = stages.get(&row.id).is_some_and(|s| *s == row.signature());
        }
        Ok(())
    }
    pub fn unresolved_exception_count(&self) -> Result<usize> {
        Ok(self.db.query_row(
            "SELECT count(*) FROM source_observations o
            WHERE disposition NOT IN ('purchase','pending','payment')
            AND NOT EXISTS(SELECT 1 FROM exception_decisions e WHERE e.observation_id=o.id)",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn queue_summary(&self) -> Result<QueueSummary> {
        QueueSummary::from_rows(&self.detail_rows()?, self.unresolved_exception_count()?)
    }
    pub(crate) fn validate_staging(&self, selected: &[(i64, String)], staged: bool) -> Result<()> {
        if selected.is_empty()
            || selected.len() > 100
            || selected.iter().map(|r| r.0).collect::<BTreeSet<_>>().len() != selected.len()
        {
            return Err(Error::Invalid("Select 1–100 distinct purchases.".into()));
        }
        let rows = self.detail_rows()?;
        for (id, signature) in selected {
            let row = rows.iter().find(|r| r.id == *id).ok_or(Error::NotFound)?;
            if row.signature() != *signature {
                return Err(Error::Stale);
            }
            if staged {
                if !row.can_stage() {
                    return Err(Error::Invalid("Complete the selected purchases before staging; handled purchases cannot be staged.".into()));
                }
                if row.handling != "outstanding" && self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM handling_decisions WHERE purchase_id=?1 AND state IN ('external','excluded'))",
                    [id], |r| r.get::<_,bool>(0))? {
                    return Err(Error::Invalid("Explicitly reopen previously handled/excluded purchases with evidence before staging.".into()));
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn save_staging(
    tx: &rusqlite::Connection,
    rows: &[DetailRow],
    staged: bool,
) -> Result<()> {
    for row in rows {
        let mut row = row.clone();
        if staged && row.handling == "unknown" {
            tx.execute("INSERT INTO handling_decisions(purchase_id,version,state,reason,reopen_confirmed,provenance)
                VALUES(?1,?2,'outstanding','User reviewed and staged this purchase as outstanding',0,'manual')",
                params![row.id,row.handling_version+1])?;
            row.handling = "outstanding".into();
            row.handling_version += 1;
        }
        tx.execute(
            "INSERT INTO staging_events(purchase_id,staged,signature,snapshot) VALUES(?1,?2,?3,?4)",
            params![row.id, staged, row.signature(), row.snapshot()],
        )?;
    }
    Ok(())
}

pub struct PurchaseEdit {
    pub id: i64,
    pub signature: String,
    pub account: String,
    pub category: String,
    pub description: String,
    pub use_category_default: bool,
    pub use_description_default: bool,
    pub allow_category: bool,
    pub stage: bool,
}

pub struct PageAccountEdit {
    pub id: i64,
    pub signature: String,
    pub original: String,
    pub account: String,
}

impl PurchaseEdit {
    fn validate(&self) -> Result<()> {
        if !matches!(self.account.as_str(), "" | "personal" | "joint")
            || self.category.len() > 200
            || self.description.len() > 4000
        {
            return Err(Error::Invalid("Invalid purchase details.".into()));
        }
        Ok(())
    }

    /// Save within the caller's transaction, preserving unchanged inherited fields.
    fn save(&self, db: &rusqlite::Connection, row: &DetailRow) -> Result<()> {
        // Leaving a rule-derived account unchanged keeps its provenance.
        if row.account != self.account {
            let account = (!self.account.is_empty()).then_some(&self.account);
            db.execute(
                "INSERT INTO review_decisions(purchase_id,version,account) VALUES(?1,?2,?3)",
                params![row.id, row.version + 1, account],
            )?;
        }

        let category = self.category.trim();
        let description = self.description.trim();
        let details_changed = row.category != category
            || row.description != description
            || self.use_category_default
            || self.use_description_default;
        if details_changed {
            let inherit_category = self.use_category_default
                || (row.category == category
                    && !row.category_source.starts_with("purchase override"));
            let inherit_description = self.use_description_default
                || (row.description == description
                    && !row.description_source.starts_with("purchase override"))
                || description.is_empty();
            let category = if inherit_category {
                None
            } else {
                crate::review::category_name(db, category, self.allow_category)?
            };
            let description = (!inherit_description).then_some(description);
            db.execute(
                "INSERT INTO purchase_details(purchase_id,version,category,description) VALUES(?1,?2,?3,?4)",
                params![row.id, row.details_version + 1, category, description],
            )?;
        }
        Ok(())
    }
}

impl Store {
    /// Only changed controls become explicit decisions. One stale change rolls back the page.
    pub fn save_page_accounts(&mut self, edits: &[PageAccountEdit]) -> Result<usize> {
        if edits.len() > 50
            || edits.iter().map(|e| e.id).collect::<BTreeSet<_>>().len() != edits.len()
        {
            return Err(Error::Invalid(
                "Save at most 50 distinct purchases on this page.".into(),
            ));
        }
        for edit in edits {
            if !matches!(edit.account.as_str(), "" | "personal" | "joint")
                || !matches!(edit.original.as_str(), "" | "personal" | "joint")
            {
                return Err(Error::Invalid("Choose unset, Personal, or Joint.".into()));
            }
        }
        let changed: Vec<_> = edits.iter().filter(|e| e.account != e.original).collect();
        if changed.is_empty() {
            return Ok(0);
        }
        self.backup()?;
        self.atomic(|store| {
            let rows = store.detail_rows()?;
            for edit in &changed {
                let row = rows
                    .iter()
                    .find(|r| r.id == edit.id)
                    .ok_or(Error::NotFound)?;
                if row.signature() != edit.signature || row.account != edit.original {
                    return Err(Error::Stale);
                }
                store.execution_editable(&[row.id])?;
                if row.conflict {
                    return Err(Error::Invalid(
                        "Resolve source conflicts before changing accounts.".into(),
                    ));
                }
                let account = (!edit.account.is_empty()).then_some(&edit.account);
                store.db.execute(
                    "INSERT INTO review_decisions(purchase_id,version,account) VALUES(?1,?2,?3)",
                    params![row.id, row.version + 1, account],
                )?;
            }
            Ok(changed.len())
        })
    }

    pub fn edit_purchase(&mut self, edit: PurchaseEdit) -> Result<()> {
        self.execution_editable(&[edit.id])?;
        edit.validate()?;
        self.backup()?;
        self.atomic(|store| {
            let row = store.detail_row(edit.id)?;
            if row.signature() != edit.signature {
                return Err(Error::Stale);
            }
            if row.conflict {
                return Err(Error::Invalid("Resolve the source conflict first.".into()));
            }
            edit.save(&store.db, &row)?;
            if edit.stage {
                let row = store.detail_row(edit.id)?;
                store.validate_staging(&[(row.id, row.signature())], true)?;
                save_staging(&store.db, &[row], true)?;
            }
            Ok(())
        })
    }
}
