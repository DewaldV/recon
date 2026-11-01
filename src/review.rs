//! Offline review: effective fields, conservative merchant identity, and atomic previews.
use crate::{Error, Result, importer::MAX_UPLOAD, storage::Store};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Policy v1: Unicode lowercase and collapsed Unicode whitespace; punctuation stays intact.
pub fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogueRow {
    pub record: i64,
    pub merchant: String,
    pub category: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Change {
    Historical {
        filename: String,
        bytes: Vec<u8>,
        rows: Vec<crate::history_import::HistoricalRow>,
    },
    Catalogue {
        filename: String,
        bytes: Vec<u8>,
        rows: Vec<CatalogueRow>,
    },
    Default {
        merchant: String,
        category: String,
        description: String,
    },
    Details {
        ids: Vec<i64>,
        category: String,
        description: String,
    },
    Alias {
        merchant: String,
        target: String,
    },
    Accounts {
        rows: Vec<(i64, i64)>,
        account: String,
    },
    Handling {
        ids: Vec<i64>,
        state: String,
        reason: String,
        reopen: bool,
    },
    CreateRecon {
        name: String,
    },
    Membership {
        ids: Vec<i64>,
        recon_id: i64,
        include: bool,
        reason: String,
    },
    ReconStatus {
        recon_id: i64,
        status: String,
        reason: String,
    },
    Staging {
        rows: Vec<(i64, String)>,
        staged: bool,
    },
    AccountRule {
        merchant: String,
        card: String,
        account: String,
    },
    Exception {
        observation_id: i64,
        reason: String,
    },
}
#[derive(Clone, Debug)]
pub struct DetailRow {
    pub id: i64,
    pub merchant: String,
    pub card: String,
    pub date: String,
    pub amount: String,
    pub amount_pence: i64,
    pub account: String,
    pub version: i64,
    pub category: String,
    pub description: String,
    pub details_version: i64,
    pub category_source: String,
    pub description_source: String,
    pub conflict: bool,
    pub handling: String,
    pub handling_version: i64,
    pub handling_reason: String,
    pub active_recon_id: Option<i64>,
    pub active_recon_name: String,
    pub account_source: String,
    pub staged: bool,
    pub in_run: bool,
    pub source_version: i64,
}
impl DetailRow {
    pub fn complete(&self) -> bool {
        !self.conflict
            && !self.account.is_empty()
            && !self.category.is_empty()
            && !self.description.is_empty()
    }
    pub fn ready(&self) -> bool {
        self.complete()
            && matches!(self.account.as_str(), "personal" | "joint")
            && self.handling == "outstanding"
            && self.active_recon_id.is_some()
            && self.amount_pence > 0
    }
    pub fn readiness(&self) -> &'static str {
        match self.handling.as_str() {
            "external" => "Handled externally · do not fund again",
            "excluded" => "Excluded · not outstanding",
            "unknown" => "Blocked · handling history unknown",
            _ if self.conflict => "Blocked · source conflict",
            _ if !self.complete() => "Blocked · missing account or merchant details",
            _ if self.active_recon_id.is_none() => "Blocked · no active recon selection",
            _ => "Outstanding-ready · not payment approval",
        }
    }
    pub fn ready_in(&self, recon_id: Option<i64>) -> bool {
        self.ready() && recon_id.is_none_or(|id| self.active_recon_id == Some(id))
    }
    pub fn readiness_in(&self, recon_id: &Option<i64>) -> &'static str {
        if self.handling == "outstanding"
            && recon_id.is_some_and(|id| self.active_recon_id != Some(id))
        {
            "Blocked · selected recon is not the active scope"
        } else {
            self.readiness()
        }
    }
}
#[derive(Default, Clone, Deserialize)]
pub struct Filters {
    #[serde(default, deserialize_with = "optional_import")]
    pub import_notice: Option<i64>,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub missing: String,
    #[serde(default)]
    pub view: String,
    #[serde(default, deserialize_with = "optional_import")]
    pub import_id: Option<i64>,
    #[serde(default, deserialize_with = "optional_import")]
    pub recon_id: Option<i64>,
    pub page: Option<usize>,
}
impl Filters {
    pub fn selected_import(&self, id: &i64) -> bool {
        self.import_id == Some(*id)
    }
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.view.as_str(),
            "" | "all"
                | "attention"
                | "complete"
                | "ready"
                | "external"
                | "excluded"
                | "unknown"
                | "staged"
                | "outstanding"
                | "history"
        ) || !matches!(self.account.as_str(), "" | "unset" | "personal" | "joint")
            || !matches!(
                self.missing.as_str(),
                "" | "account" | "category" | "description" | "conflict" | "handling" | "recon"
            )
        {
            return Err(Error::Invalid("Invalid review filter.".into()));
        }
        Ok(())
    }
}
fn optional_import<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<i64>, D::Error> {
    let value = String::deserialize(d)?;
    if value.is_empty() {
        Ok(None)
    } else {
        value.parse().map(Some).map_err(serde::de::Error::custom)
    }
}
pub struct ReviewPage {
    pub rows: Vec<DetailRow>,
    pub total: usize,
    pub missing_account: usize,
    pub missing_category: usize,
    pub missing_description: usize,
    pub conflicts: usize,
    pub page: usize,
    pub pages: usize,
    pub groups: Vec<(String, usize)>,
    pub categories: Vec<String>,
    pub queue: crate::queue::QueueSummary,
    pub imports: Vec<(i64, String)>,
    pub active_run: Option<i64>,
}
pub struct Preview {
    pub token: String,
    pub change: Change,
    pub affected: Vec<DetailRow>,
    pub preserved: usize,
    pub catalogue: Vec<(CatalogueRow, String, bool)>,
    pub duplicate: bool,
    pub return_to: String,
    pub historical: Vec<crate::history_import::HistoricalRow>,
    pub rule_impact: Option<crate::rules::RuleImpact>,
}
impl Store {
    pub fn detail_rows(&self) -> Result<Vec<DetailRow>> {
        let mut stmt=self.db.prepare("SELECT p.id,p.merchant,p.card_used,p.transaction_date,p.amount_pence,coalesce(a.account,''),coalesce(a.version,0),coalesce(d.category,''),coalesce(d.description,''),EXISTS(SELECT 1 FROM source_observations o WHERE o.purchase_id=p.id AND o.disposition='conflict' AND NOT EXISTS(SELECT 1 FROM exception_decisions e WHERE e.observation_id=o.id)),coalesce(d.version,0),coalesce((SELECT max(e.id) FROM exception_decisions e JOIN source_observations o ON o.id=e.observation_id WHERE o.purchase_id=p.id),0) FROM purchases p LEFT JOIN current_account_decisions a ON a.purchase_id=p.id LEFT JOIN current_details d ON d.purchase_id=p.id ORDER BY p.transaction_date DESC,p.id DESC")?;
        let mut rows = stmt
            .query_map([], |r| {
                let amount: i64 = r.get(4)?;
                Ok(DetailRow {
                    id: r.get(0)?,
                    merchant: r.get(1)?,
                    card: r.get(2)?,
                    date: r.get(3)?,
                    amount: format!("{}.{:02}", amount / 100, amount % 100),
                    amount_pence: amount,
                    account: r.get(5)?,
                    version: r.get(6)?,
                    category: r.get(7)?,
                    description: r.get(8)?,
                    conflict: r.get(9)?,
                    details_version: r.get(10)?,
                    category_source: String::new(),
                    description_source: String::new(),
                    handling: "unknown".into(),
                    handling_version: 0,
                    handling_reason: String::new(),
                    active_recon_id: None,
                    active_recon_name: String::new(),
                    account_source: String::new(),
                    staged: false,
                    in_run: false,
                    source_version: r.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let definitions = self.definitions()?;
        let aliases = self.aliases()?;
        for row in &mut rows {
            let key = normalize(&row.merchant);
            let key = aliases.get(&key).unwrap_or(&key);
            let default = definitions.get(key);
            for (field, source, index) in [
                (&mut row.category, &mut row.category_source, 0),
                (&mut row.description, &mut row.description_source, 1),
            ] {
                if !field.is_empty() {
                    *source = format!("purchase override v{}", row.details_version);
                } else if let Some((category, description, version)) = default {
                    *field = if index == 0 {
                        category.clone()
                    } else {
                        description.clone()
                    };
                    *source = if field.is_empty() {
                        "missing".into()
                    } else {
                        format!("merchant default v{version} · {key}")
                    };
                } else {
                    *source = "missing".into();
                }
            }
        }
        self.enrich_handling(&mut rows)?;
        self.enrich_accounts(&mut rows)?;
        self.enrich_execution(&mut rows)?;
        self.enrich_staging(&mut rows)?;
        Ok(rows)
    }

    pub fn detail_row(&self, id: i64) -> Result<DetailRow> {
        self.detail_rows()?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(Error::NotFound)
    }

    pub fn merchant_default(&self, merchant: &str) -> Result<(String, String)> {
        let key = normalize(merchant);
        let aliases = self.aliases()?;
        let key = aliases.get(&key).unwrap_or(&key);
        Ok(self
            .definitions()?
            .get(key)
            .map(|(c, d, _)| (c.clone(), d.clone()))
            .unwrap_or_default())
    }
    fn definitions(&self) -> Result<BTreeMap<String, (String, String, i64)>> {
        Ok(self.db.prepare("SELECT merchant_key,coalesce(category,''),coalesce(description,''),version FROM current_merchants")?.query_map([],|r| Ok((r.get(0)?,(r.get(1)?,r.get(2)?,r.get(3)?))))?.collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn aliases(&self) -> Result<BTreeMap<String, String>> {
        Ok(self
            .db
            .prepare("SELECT alias_key,target_key FROM merchant_aliases")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn categories(&self) -> Result<Vec<String>> {
        Ok(self
            .db
            .prepare("SELECT name FROM categories ORDER BY name")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn review_page(&self, filter: &Filters) -> Result<ReviewPage> {
        filter.validate()?;
        let members:Option<BTreeSet<i64>>=filter.import_id.map(|id|->Result<_>{ Ok(self.db.prepare("SELECT purchase_id FROM source_observations WHERE import_id=?1 AND purchase_id IS NOT NULL UNION SELECT n.purchase_id FROM observation_repairs r JOIN source_observations o ON o.id=r.observation_id JOIN source_observations n ON n.id=r.replacement_observation_id WHERE o.import_id=?1 AND n.purchase_id IS NOT NULL")?.query_map([id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?) }).transpose()?;
        let recon_members = filter
            .recon_id
            .map(|id| self.recon_members(id))
            .transpose()?;
        let all = self.detail_rows()?;
        let queue =
            crate::queue::QueueSummary::from_rows(&all, self.unresolved_exception_count()?)?;
        let rows: Vec<_> = all
            .into_iter()
            .filter(|r| {
                (filter.q.is_empty() || normalize(&r.merchant).contains(&normalize(&filter.q)))
                    && (filter.account.is_empty()
                        || if filter.account == "unset" {
                            r.account.is_empty()
                        } else {
                            r.account == filter.account
                        })
                    && members.as_ref().is_none_or(|m| m.contains(&r.id))
                    && recon_members.as_ref().is_none_or(|m| m.contains(&r.id))
                    && match filter.view.as_str() {
                        "attention" => r.outstanding() && !r.staged,
                        "staged" => r.eligibility() == crate::queue::Eligibility::Staged,
                        "outstanding" => r.outstanding(),
                        "history" => !r.outstanding(),
                        "complete" => r.complete(),
                        "ready" => r.ready_in(filter.recon_id),
                        "external" => r.handling == "external",
                        "excluded" => r.handling == "excluded",
                        "unknown" => r.handling == "unknown",
                        _ => true,
                    }
                    && match filter.missing.as_str() {
                        "account" => r.account.is_empty(),
                        "category" => r.category.is_empty(),
                        "description" => r.description.is_empty(),
                        "conflict" => r.conflict,
                        "handling" => r.handling == "unknown",
                        "recon" => r.active_recon_id.is_none(),
                        _ => true,
                    }
            })
            .collect();
        let mut groups = BTreeMap::new();
        for row in &rows {
            if row.category.is_empty() || row.description.is_empty() {
                *groups.entry(row.merchant.clone()).or_insert(0) += 1;
            }
        }
        let total = rows.len();
        let pages = total.div_ceil(50).max(1);
        let page = filter.page.unwrap_or(1).clamp(1, pages);
        Ok(ReviewPage {
            missing_account: rows.iter().filter(|r| r.account.is_empty()).count(),
            missing_category: rows.iter().filter(|r| r.category.is_empty()).count(),
            missing_description: rows.iter().filter(|r| r.description.is_empty()).count(),
            conflicts: rows.iter().filter(|r| r.conflict).count(),
            rows: rows.into_iter().skip((page - 1) * 50).take(50).collect(),
            total,
            page,
            pages,
            groups: groups.into_iter().collect(),
            categories: self.categories()?,
            queue,
            imports: self.recent_imports()?,
            active_run: self.active_run()?,
        })
    }
    pub fn parse_catalogue(filename: String, bytes: Vec<u8>) -> Result<Change> {
        if bytes.len() > MAX_UPLOAD || filename.len() > 255 {
            return Err(Error::Invalid("Catalogue exceeds upload limits.".into()));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Invalid("Catalogue must be UTF-8.".into()))?
            .trim_start_matches('\u{feff}');
        let mut reader = csv::Reader::from_reader(text.as_bytes());
        let header = reader
            .headers()
            .map_err(|_| Error::Invalid("Invalid catalogue header.".into()))?
            .clone();
        let expected = ["Merchant", "Category", "Description"];
        if header.len() != 3
            || expected
                .iter()
                .any(|h| header.iter().filter(|x| x == h).count() != 1)
        {
            return Err(Error::Invalid(
                "Expected Merchant, Category, Description columns.".into(),
            ));
        }
        let ix: Vec<_> = expected
            .iter()
            .map(|h| header.iter().position(|x| x == *h).unwrap())
            .collect();
        let mut rows = Vec::new();
        for (i, row) in reader.records().enumerate() {
            let row =
                row.map_err(|_| Error::Invalid("Invalid catalogue row; no changes saved.".into()))?;
            if normalize(&row[ix[0]]).is_empty() {
                return Err(Error::Invalid(
                    "Every catalogue row needs a merchant name.".into(),
                ));
            }
            validate_text(&row[ix[0]], &row[ix[1]], &row[ix[2]])?;
            rows.push(CatalogueRow {
                record: i as i64 + 2,
                merchant: row[ix[0]].to_owned(),
                category: row[ix[1]].trim().to_owned(),
                description: row[ix[2]].trim().to_owned(),
            });
            if rows.len() > 10000 {
                return Err(Error::Invalid(
                    "Catalogue is limited to 10,000 records.".into(),
                ));
            }
        }
        Ok(Change::Catalogue {
            filename,
            bytes,
            rows,
        })
    }
    pub fn stage_review(&mut self, change: Change) -> Result<String> {
        self.validate_change(&change)?;
        let payload = serde_json::to_string(&change).map_err(|_| Error::Worker)?;
        let token = crate::config::random_secret()?;
        self.db.execute("INSERT INTO review_previews(token,revision,payload) SELECT ?1,revision,?2 FROM review_revision WHERE id=1",params![token,payload])?;
        Ok(token)
    }
    fn validate_change(&self, change: &Change) -> Result<()> {
        match change {
            Change::Historical { rows, .. } => self.validate_history(rows)?,
            Change::Details { ids, .. } | Change::Handling { ids, .. } => {
                self.execution_editable(ids)?
            }
            Change::Accounts { rows, .. } => {
                self.execution_editable(&rows.iter().map(|r| r.0).collect::<Vec<_>>())?
            }
            Change::Staging { rows, .. } => {
                self.execution_editable(&rows.iter().map(|r| r.0).collect::<Vec<_>>())?
            }
            _ => {}
        }
        match change {
            Change::Staging { rows, staged } => self.validate_staging(rows, *staged)?,
            Change::AccountRule {
                merchant,
                card,
                account,
            } => {
                crate::rules::validate_rule(merchant, card, account)?;
                if self.aliases()?.contains_key(&normalize(merchant)) {
                    return Err(Error::Invalid(
                        "Use the alias target's exact merchant name for account rules.".into(),
                    ));
                }
            }
            Change::Exception {
                observation_id,
                reason,
            } => {
                if reason.trim().is_empty() || reason.len() > 4000 {
                    return Err(Error::Invalid(
                        "Provide an exception disposition reason.".into(),
                    ));
                }
                if !self.db.query_row("SELECT EXISTS(SELECT 1 FROM source_observations o WHERE id=?1 AND disposition NOT IN ('purchase','pending','payment') AND NOT EXISTS(SELECT 1 FROM exception_decisions e WHERE e.observation_id=o.id))", [observation_id], |r|r.get::<_,bool>(0))? {
                    return Err(Error::Invalid("This observation does not need a disposition.".into()));
                }
            }
            Change::Catalogue { .. } | Change::Historical { .. } => {}
            Change::Default {
                merchant,
                category,
                description,
            } => {
                validate_text(merchant, category, description)?;
                if self.aliases()?.contains_key(&normalize(merchant)) {
                    return Err(Error::Invalid(
                        "Edit the alias target's merchant default.".into(),
                    ));
                }
            }
            Change::Details {
                ids,
                category,
                description,
            } => {
                validate_text("purchase", category, description)?;
                self.validate_ids(ids)?;
            }
            Change::Accounts { rows, account } => {
                if !matches!(account.as_str(), "" | "personal" | "joint") {
                    return Err(Error::Invalid("Invalid account.".into()));
                }
                self.validate_ids(&rows.iter().map(|r| r.0).collect::<Vec<_>>())?;
                let current = self.detail_rows()?;
                for (id, version) in rows {
                    if !current.iter().any(|r| r.id == *id && r.version == *version) {
                        return Err(Error::Stale);
                    }
                }
            }
            Change::Alias { merchant, target } => {
                validate_text(merchant, "", target)?;
                let key = normalize(merchant);
                let target = normalize(target);
                let definitions = self.definitions()?;
                let aliases = self.aliases()?;
                if key == target
                    || definitions.contains_key(&key)
                    || aliases.contains_key(&key)
                    || aliases.contains_key(&target)
                    || aliases.values().any(|v| v == &key)
                    || !definitions.contains_key(&target)
                {
                    return Err(Error::Invalid("Alias needs a new exact name and an existing direct merchant default; chains and conflicting definitions are not allowed.".into()));
                }
            }
            _ => self.validate_recon_change(change)?,
        }
        Ok(())
    }
    fn validate_ids(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty()
            || ids.len() > 100
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error::Invalid("Select 1–100 distinct purchases.".into()));
        }
        let rows = self.detail_rows()?;
        for id in ids {
            let row = rows.iter().find(|r| r.id == *id).ok_or(Error::NotFound)?;
            if row.conflict {
                return Err(Error::Invalid(
                    "Selected purchase has unresolved source conflicts.".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn review_preview(&self, token: &str) -> Result<Preview> {
        let (revision, payload, committed): (i64, String, i64) = self
            .db
            .query_row(
                "SELECT revision,payload,committed FROM review_previews WHERE token=?1",
                [token],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let current: i64 =
            self.db
                .query_row("SELECT revision FROM review_revision WHERE id=1", [], |r| {
                    r.get(0)
                })?;
        if revision != current || committed != 0 {
            return Err(Error::Stale);
        }
        let change: Change = serde_json::from_str(&payload).map_err(|_| Error::Worker)?;
        let definitions = self.definitions()?;
        let aliases = self.aliases()?;
        let mut catalogue = Vec::new();
        let mut duplicate = false;
        let mut keys = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut historical = Vec::new();
        match &change {
            Change::Historical { bytes, rows, .. } => {
                duplicate = self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM historical_sources WHERE sha256=?1)",
                    [format!("{:x}", Sha256::digest(bytes))],
                    |r| r.get(0),
                )?;
                ids.extend(rows.iter().map(|r| r.purchase_id));
                historical = rows.clone();
            }
            Change::Staging { rows, .. } => ids.extend(rows.iter().map(|r| r.0)),
            Change::AccountRule { merchant, .. } => {
                keys.insert(normalize(merchant));
            }
            Change::Exception { .. } => {}
            Change::Catalogue { bytes, rows, .. } => {
                duplicate = self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM merchant_sources WHERE sha256=?1)",
                    [format!("{:x}", Sha256::digest(bytes))],
                    |r| r.get(0),
                )?;
                let mut variants: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
                for r in rows {
                    variants
                        .entry(normalize(&r.merchant))
                        .or_default()
                        .insert((r.category.clone(), r.description.clone()));
                }
                let mut seen = BTreeSet::new();
                for r in rows {
                    let key = normalize(&r.merchant);
                    keys.insert(key.clone());
                    let conflict = variants[&key].len() > 1
                        || aliases.contains_key(&key)
                        || definitions
                            .get(&key)
                            .is_some_and(|(c, d, _)| c != &r.category || d != &r.description);
                    let status = if duplicate {
                        "Already imported; later edits preserved"
                    } else if aliases.contains_key(&key) {
                        "Alias exists; resolve via target, leave unchecked"
                    } else if conflict {
                        "Conflict: explicitly select at most one definition"
                    } else if definitions.contains_key(&key) {
                        "Unchanged"
                    } else if r.category.is_empty() || r.description.is_empty() {
                        "Incomplete new definition"
                    } else {
                        "New definition"
                    };
                    let selected = !duplicate
                        && !conflict
                        && seen.insert((key, r.category.clone(), r.description.clone()));
                    catalogue.push((r.clone(), status.into(), selected));
                }
            }
            Change::Default { merchant, .. } => {
                keys.insert(normalize(merchant));
            }
            Change::Alias { merchant, .. } => {
                keys.insert(normalize(merchant));
            }
            Change::Details { ids: selected, .. } => ids.extend(selected),
            Change::Accounts { rows, .. } => ids.extend(rows.iter().map(|r| r.0)),
            Change::Handling { ids: selected, .. } | Change::Membership { ids: selected, .. } => {
                ids.extend(selected)
            }
            Change::CreateRecon { .. } => {}
            Change::ReconStatus { recon_id, .. } => ids.extend(self.recon_members(*recon_id)?),
        }
        let affected: Vec<_> = self
            .detail_rows()?
            .into_iter()
            .filter(|r| {
                let key = normalize(&r.merchant);
                let merchant_matches = ids.contains(&r.id)
                    || keys.contains(&key)
                    || aliases.get(&key).is_some_and(|k| keys.contains(k));
                merchant_matches
                    && match &change {
                        Change::AccountRule { card, .. } => {
                            card.trim().is_empty() || r.card == card.trim()
                        }
                        _ => true,
                    }
            })
            .collect();
        let preserved = affected
            .iter()
            .filter(|r| {
                r.category_source.starts_with("purchase override")
                    || r.description_source.starts_with("purchase override")
            })
            .count();
        let rule_impact = match &change {
            Change::AccountRule {
                merchant,
                card,
                account,
            } => Some(self.rule_impact(&affected, merchant, card, account)?),
            _ => None,
        };
        Ok(Preview {
            token: token.into(),
            change,
            affected,
            preserved,
            catalogue,
            duplicate,
            historical,
            rule_impact,
            return_to: self
                .db
                .query_row(
                    "SELECT return_to FROM preview_context WHERE token=?1",
                    [token],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or_else(|| "/review".into()),
        })
    }
    pub fn commit_review(
        &mut self,
        token: &str,
        selected: &BTreeSet<i64>,
        allow_categories: bool,
    ) -> Result<()> {
        let preview = self.review_preview(token)?;
        self.validate_change(&preview.change)?;
        if !preview.duplicate {
            self.backup()?;
        }
        // The process lock and serialized worker exclude app writers; the transaction also
        // rejects external writes between revision validation and commit.
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let revision: i64 =
            tx.query_row("SELECT revision FROM review_revision WHERE id=1", [], |r| {
                r.get(0)
            })?;
        let expected: i64 = tx.query_row(
            "SELECT revision FROM review_previews WHERE token=?1",
            [token],
            |r| r.get(0),
        )?;
        if revision != expected {
            return Err(Error::Stale);
        }
        match preview.change {
            Change::Historical {
                filename,
                bytes,
                rows,
            } => {
                if !preview.duplicate {
                    crate::history_import::save_history(
                        &tx,
                        &filename,
                        &bytes,
                        &rows,
                        &preview.affected,
                        allow_categories,
                    )?;
                }
            }
            Change::Staging { staged, .. } => {
                crate::queue::save_staging(&tx, &preview.affected, staged)?
            }
            Change::AccountRule {
                merchant,
                card,
                account,
            } => crate::rules::save_rule(&tx, &merchant, &card, &account)?,
            Change::Exception {
                observation_id,
                reason,
            } => {
                tx.execute(
                    "INSERT INTO exception_decisions(observation_id,reason) VALUES(?1,?2)",
                    params![observation_id, reason.trim()],
                )?;
            }
            Change::Catalogue {
                filename,
                bytes,
                rows,
            } => {
                if !preview.duplicate {
                    let mut unique = BTreeMap::new();
                    for r in rows.iter().filter(|r| selected.contains(&r.record)) {
                        let key = normalize(&r.merchant);
                        if let Some(previous) =
                            unique.insert(key, (r.category.clone(), r.description.clone()))
                            && previous != (r.category.clone(), r.description.clone())
                        {
                            return Err(Error::Invalid(
                                "Choose at most one distinct definition per normalized merchant."
                                    .into(),
                            ));
                        }
                    }
                    if selected
                        .iter()
                        .any(|id| !rows.iter().any(|r| r.record == *id))
                    {
                        return Err(Error::Invalid("Invalid catalogue selection.".into()));
                    }
                    tx.execute(
                        "INSERT INTO merchant_sources(sha256,filename,bytes) VALUES(?1,?2,?3)",
                        params![format!("{:x}", Sha256::digest(&bytes)), filename, bytes],
                    )?;
                    let source = tx.last_insert_rowid();
                    let mut saved = BTreeSet::new();
                    for r in rows.iter().filter(|r| selected.contains(&r.record)) {
                        if !saved.insert(normalize(&r.merchant)) {
                            continue;
                        }
                        if tx.query_row(
                            "SELECT EXISTS(SELECT 1 FROM merchant_aliases WHERE alias_key=?1)",
                            [normalize(&r.merchant)],
                            |r| r.get::<_, bool>(0),
                        )? {
                            return Err(Error::Invalid(
                                "An alias cannot be replaced by a catalogue definition.".into(),
                            ));
                        }
                        save_default(
                            &tx,
                            &r.merchant,
                            &r.category,
                            &r.description,
                            Some((source, r.record)),
                            allow_categories,
                        )?;
                    }
                }
            }
            Change::Default {
                merchant,
                category,
                description,
            } => save_default(
                &tx,
                &merchant,
                &category,
                &description,
                None,
                allow_categories,
            )?,
            Change::Details {
                ids,
                category,
                description,
            } => {
                let category = category_name(&tx, &category, allow_categories)?;
                for id in ids {
                    tx.execute("INSERT INTO purchase_details(purchase_id,version,category,description) SELECT ?1,coalesce(max(version),0)+1,?2,?3 FROM purchase_details WHERE purchase_id=?1",params![id,category,clean(&description)])?;
                }
            }
            Change::Alias { merchant, target } => {
                tx.execute("INSERT INTO merchant_aliases(alias_key,target_key,original_name) VALUES(?1,?2,?3)",params![normalize(&merchant),normalize(&target),merchant])?;
            }
            Change::Accounts { rows, account } => {
                for (id, version) in rows {
                    tx.execute("INSERT INTO review_decisions(purchase_id,version,account) VALUES(?1,?2,?3)",params![id,version+1,clean(&account)])?;
                }
            }
            other => Store::apply_recon_change(&tx, other)?,
        }
        tx.execute(
            "UPDATE review_previews SET committed=1,committed_at=CURRENT_TIMESTAMP WHERE token=?1",
            [token],
        )?;
        tx.commit()?;
        Ok(())
    }
}
fn validate_text(merchant: &str, category: &str, description: &str) -> Result<()> {
    if normalize(merchant).is_empty()
        || merchant.len() > 1000
        || category.len() > 200
        || description.len() > 4000
    {
        return Err(Error::Invalid("Merchant is required; limits are 1,000/200/4,000 bytes for merchant/category/description.".into()));
    }
    Ok(())
}
pub(crate) fn category_name(
    tx: &rusqlite::Connection,
    value: &str,
    allow: bool,
) -> Result<Option<String>> {
    let Some(value) = clean(value) else {
        return Ok(None);
    };
    let key = normalize(&value);
    if let Some(existing) = tx
        .query_row(
            "SELECT name FROM categories WHERE normalized=?1",
            [&key],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(Some(existing));
    }
    if !allow {
        return Err(Error::Invalid(
            "Approve adding the previewed categories to the local catalogue.".into(),
        ));
    }
    tx.execute(
        "INSERT INTO categories(name,normalized) VALUES(?1,?2)",
        params![value, key],
    )?;
    Ok(Some(value))
}
fn save_default(
    tx: &rusqlite::Transaction<'_>,
    merchant: &str,
    category: &str,
    description: &str,
    source: Option<(i64, i64)>,
    allow: bool,
) -> Result<()> {
    validate_text(merchant, category, description)?;
    let key = normalize(merchant);
    let category = category_name(tx, category, allow)?;
    let description = clean(description);
    let existing: Option<(Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT category,description FROM current_merchants WHERE merchant_key=?1",
            [&key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if existing == Some((category.clone(), description.clone())) {
        return Ok(());
    }
    tx.execute("INSERT INTO merchant_definitions(merchant_key,original_name,version,category,description,source_id,source_record) SELECT ?1,?2,coalesce(max(version),0)+1,?3,?4,?5,?6 FROM merchant_definitions WHERE merchant_key=?1",params![key,merchant,category,description,source.map(|s|s.0),source.map(|s|s.1)])?;
    Ok(())
}
