//! Offline handling and recon scope. Details-complete is never proof of funding.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Transaction, params};

use crate::{
    Error, Result,
    review::{Change, DetailRow, normalize},
    storage::Store,
};

#[derive(Clone, Debug)]
pub struct ReconInfo {
    pub id: i64,
    pub name: String,
    pub status: String,
}

pub struct ReconSummary {
    pub recon: ReconInfo,
    pub total: usize,
    pub ready: usize,
    pub blocked: usize,
    pub unknown: usize,
    pub external: usize,
    pub excluded: usize,
    pub personal_count: usize,
    pub joint_count: usize,
    pub personal_amount: String,
    pub joint_amount: String,
    pub status_history: Vec<HistoryEvent>,
}

pub struct HistoryEvent {
    pub at: String,
    pub kind: String,
    pub value: String,
    pub reason: String,
}

pub fn format_pennies(amount: i64) -> String {
    format!("{}.{:02}", amount / 100, amount % 100)
}

impl Store {
    pub fn recons(&self) -> Result<Vec<ReconInfo>> {
        Ok(self.db.prepare("SELECT r.id,r.name,s.status FROM recons r JOIN current_recon_status s ON s.recon_id=r.id ORDER BY r.id DESC")?
            .query_map([], |r| Ok(ReconInfo { id:r.get(0)?, name:r.get(1)?, status:r.get(2)? }))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn recon_info(&self, id: i64) -> Result<ReconInfo> {
        self.recons()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(Error::NotFound)
    }

    pub fn recon_members(&self, id: i64) -> Result<BTreeSet<i64>> {
        self.recon_info(id)?;
        Ok(self
            .db
            .prepare("SELECT purchase_id FROM current_recon_memberships WHERE recon_id=?1")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn enrich_handling(&self, rows: &mut [DetailRow]) -> Result<()> {
        let handling: BTreeMap<i64, (String, i64, String)> = self
            .db
            .prepare("SELECT purchase_id,state,version,reason FROM current_handling")?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?
            .collect::<rusqlite::Result<_>>()?;
        let membership: BTreeMap<i64,(i64,String)> = self.db.prepare("SELECT m.purchase_id,r.id,r.name FROM current_recon_memberships m JOIN recons r ON r.id=m.recon_id JOIN current_recon_status s ON s.recon_id=r.id WHERE s.status='active'")?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?,r.get(2)?))))?.collect::<rusqlite::Result<_>>()?;
        for row in rows {
            if let Some((state, version, reason)) = handling.get(&row.id) {
                row.handling = state.clone();
                row.handling_version = *version;
                row.handling_reason = reason.clone();
            }
            if let Some((id, name)) = membership.get(&row.id) {
                row.active_recon_id = Some(*id);
                row.active_recon_name = name.clone();
            }
        }
        Ok(())
    }

    pub fn recon_summary(&self, id: i64) -> Result<ReconSummary> {
        let recon = self.recon_info(id)?;
        let members = self.recon_members(id)?;
        let status_history = self.db.prepare("SELECT created_at,status,reason FROM recon_status_events WHERE recon_id=?1 ORDER BY version DESC")?
            .query_map([id], |r| Ok(HistoryEvent { at:r.get(0)?, kind:"recon status".into(), value:r.get(1)?, reason:r.get(2)? }))?.collect::<rusqlite::Result<_>>()?;
        let mut result = ReconSummary {
            recon,
            total: members.len(),
            ready: 0,
            blocked: 0,
            unknown: 0,
            external: 0,
            excluded: 0,
            personal_count: 0,
            joint_count: 0,
            personal_amount: String::new(),
            joint_amount: String::new(),
            status_history,
        };
        let (mut personal, mut joint) = (0_i64, 0_i64);
        for row in self
            .detail_rows()?
            .into_iter()
            .filter(|r| members.contains(&r.id))
        {
            match row.handling.as_str() {
                "external" => result.external += 1,
                "excluded" => result.excluded += 1,
                "unknown" => result.unknown += 1,
                "outstanding" if row.ready() && row.active_recon_id == Some(id) => {
                    result.ready += 1;
                    let sum = match row.account.as_str() {
                        "personal" => {
                            result.personal_count += 1;
                            &mut personal
                        }
                        "joint" => {
                            result.joint_count += 1;
                            &mut joint
                        }
                        _ => {
                            return Err(Error::Invalid(
                                "Invalid ready account; totals blocked.".into(),
                            ));
                        }
                    };
                    *sum = sum.checked_add(row.amount_pence).ok_or_else(|| Error::Invalid("Recon total exceeds the supported integer-penny range. No total or export is available.".into()))?;
                }
                _ => result.blocked += 1,
            }
        }
        result.personal_amount = format_pennies(personal);
        result.joint_amount = format_pennies(joint);
        Ok(result)
    }

    pub fn purchase_history(&self, id: i64) -> Result<Vec<HistoryEvent>> {
        if !self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM purchases WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::NotFound);
        }
        Ok(self.db.prepare("SELECT created_at, 'handling (manual)', state || ' v' || version || CASE WHEN reopen_confirmed=1 THEN ' · reopening confirmed' ELSE '' END, reason FROM handling_decisions WHERE purchase_id=?1
            UNION ALL SELECT m.created_at, 'recon membership', r.name || ': ' || m.action || ' v' || m.version, m.reason FROM recon_membership_events m JOIN recons r ON r.id=m.recon_id WHERE m.purchase_id=?1
            UNION ALL SELECT decided_at, 'account', coalesce(account,'unset') || ' v' || version, '' FROM review_decisions WHERE purchase_id=?1 ORDER BY 1 DESC,2,3")?
            .query_map([id], |r| Ok(HistoryEvent { at:r.get(0)?, kind:r.get(1)?, value:r.get(2)?, reason:r.get(3)? }))?.collect::<rusqlite::Result<_>>()?)
    }

    fn validate_recon_ids(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty()
            || ids.len() > 100
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error::Invalid("Select 1–100 distinct purchases.".into()));
        }
        for id in ids {
            if !self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM purchases WHERE id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::NotFound);
            }
        }
        Ok(())
    }

    pub(crate) fn validate_recon_change(&self, change: &Change) -> Result<()> {
        match change {
            Change::Handling {
                ids,
                state,
                reason,
                reopen,
            } => {
                self.validate_recon_ids(ids)?;
                valid_reason(reason)?;
                if !matches!(
                    state.as_str(),
                    "unknown" | "outstanding" | "external" | "excluded"
                ) {
                    return Err(Error::Invalid("Invalid handling state.".into()));
                }
                if state == "outstanding" && !reopen {
                    for id in ids {
                        if self.db.query_row("SELECT EXISTS(SELECT 1 FROM handling_decisions WHERE purchase_id=?1 AND state IN ('external','excluded'))", [id], |r| r.get::<_,bool>(0))? {
                            return Err(Error::Invalid("Previously handled/excluded work requires explicit reopening confirmation and a reason. This can fund the purchase twice.".into()));
                        }
                    }
                }
            }
            Change::CreateRecon { name } => {
                if name.trim().is_empty() || name.len() > 200 {
                    return Err(Error::Invalid("Recon name must be 1–200 bytes.".into()));
                }
                if self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM recons WHERE name_key=?1)",
                    [normalize(name)],
                    |r| r.get::<_, bool>(0),
                )? {
                    return Err(Error::Invalid(
                        "A recon with that name already exists, including archived history.".into(),
                    ));
                }
            }
            Change::Membership {
                ids,
                recon_id,
                include,
                reason,
            } => {
                self.validate_recon_ids(ids)?;
                valid_reason(reason)?;
                let recon = self.recon_info(*recon_id)?;
                if recon.status != "active" {
                    return Err(Error::Invalid(
                        "Reopen the archived recon before changing its selection.".into(),
                    ));
                }
                let members = self.recon_members(*recon_id)?;
                for id in ids {
                    if *include && self.db.query_row("SELECT EXISTS(SELECT 1 FROM current_recon_memberships m JOIN current_recon_status s ON s.recon_id=m.recon_id WHERE m.purchase_id=?1 AND m.recon_id!=?2 AND s.status='active')", params![id,recon_id], |r| r.get::<_,bool>(0))? {
                        return Err(Error::Invalid("A selected purchase already belongs to another active recon. Remove it there first or archive that recon.".into()));
                    }
                    if !include && !members.contains(id) {
                        return Err(Error::Invalid(
                            "A selected purchase is not a member of this recon.".into(),
                        ));
                    }
                }
            }
            Change::ReconStatus {
                recon_id,
                status,
                reason,
            } => {
                valid_reason(reason)?;
                self.recon_info(*recon_id)?;
                if !matches!(status.as_str(), "active" | "archived") {
                    return Err(Error::Invalid("Invalid recon status.".into()));
                }
                if status == "active" {
                    for id in self.recon_members(*recon_id)? {
                        if self.db.query_row("SELECT EXISTS(SELECT 1 FROM current_recon_memberships m JOIN current_recon_status s ON s.recon_id=m.recon_id WHERE m.purchase_id=?1 AND m.recon_id!=?2 AND s.status='active')", params![id,recon_id], |r| r.get::<_,bool>(0))? {
                            return Err(Error::Invalid("Reopening would duplicate another active recon's selection.".into()));
                        }
                    }
                }
            }
            _ => return Err(Error::Invalid("Not a recon action.".into())),
        }
        Ok(())
    }

    pub(crate) fn apply_recon_change(tx: &Transaction<'_>, change: Change) -> Result<()> {
        match change {
            Change::Handling {
                ids,
                state,
                reason,
                reopen,
            } => {
                for id in ids {
                    tx.execute("INSERT INTO handling_decisions(purchase_id,version,state,reason,reopen_confirmed,provenance) SELECT ?1,coalesce(max(version),0)+1,?2,?3,?4,'manual' FROM handling_decisions WHERE purchase_id=?1", params![id,state,reason.trim(),reopen])?;
                }
            }
            Change::CreateRecon { name } => {
                tx.execute(
                    "INSERT INTO recons(name,name_key) VALUES(?1,?2)",
                    params![name.trim(), normalize(&name)],
                )?;
                let id = tx.last_insert_rowid();
                tx.execute("INSERT INTO recon_status_events(recon_id,version,status,reason) VALUES(?1,1,'active','Created through an approved preview')", [id])?;
            }
            Change::Membership {
                ids,
                recon_id,
                include,
                reason,
            } => {
                for id in ids {
                    let member: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM current_recon_memberships WHERE recon_id=?1 AND purchase_id=?2)", params![recon_id,id], |r| r.get(0))?;
                    if member == include {
                        continue;
                    }
                    tx.execute("INSERT INTO recon_membership_events(recon_id,purchase_id,version,action,reason) SELECT ?1,?2,coalesce(max(version),0)+1,?3,?4 FROM recon_membership_events WHERE recon_id=?1 AND purchase_id=?2", params![recon_id,id,if include {"add"} else {"remove"},reason.trim()])?;
                }
            }
            Change::ReconStatus {
                recon_id,
                status,
                reason,
            } => {
                tx.execute("INSERT INTO recon_status_events(recon_id,version,status,reason) SELECT ?1,coalesce(max(version),0)+1,?2,?3 FROM recon_status_events WHERE recon_id=?1", params![recon_id,status,reason.trim()])?;
            }
            _ => return Err(Error::Invalid("Not a recon action.".into())),
        }
        Ok(())
    }
}

fn valid_reason(reason: &str) -> Result<()> {
    if reason.trim().is_empty() || reason.len() > 4000 {
        return Err(Error::Invalid(
            "A reason/evidence note of 1–4,000 bytes is required.".into(),
        ));
    }
    Ok(())
}
