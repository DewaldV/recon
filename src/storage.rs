use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::{
    Error, Result, config,
    importer::{self, Purchase},
};

const MIGRATIONS: [&str; 3] = [
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_original_currency.sql"),
    include_str!("../migrations/0003_merchant_review.sql"),
];

pub struct Store {
    pub(crate) db: Connection,
    directory: PathBuf,
    _lock: File,
}

#[derive(Debug)]
pub struct PurchaseRow {
    pub id: i64,
    pub reference: String,
    pub transaction_date: String,
    pub amount: String,
    pub merchant: String,
    pub card_used: String,
    pub account: String,
    pub version: i64,
    pub conflict: bool,
}

pub struct PurchasePage {
    pub rows: Vec<PurchaseRow>,
    pub page: i64,
    pub pages: i64,
    pub total: i64,
}

#[derive(Debug)]
pub struct ImportSummary {
    pub id: i64,
    pub filename: String,
    pub counts: Vec<(String, i64)>,
    pub exceptions: Vec<Exception>,
    pub total: i64,
}

#[derive(Debug)]
pub struct Exception {
    pub record_number: i64,
    pub disposition: String,
    pub reason: String,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        config::private_directory(directory)?;
        let lock = config::private_file(&directory.join("recon.lock"))?;
        lock.try_lock_exclusive().map_err(|_| {
            Error::Invalid("Another Recon process is using this data directory.".into())
        })?;
        let path = directory.join("recon.sqlite3");
        config::private_file(&path)?;
        let db = Connection::open(&path)?;
        db.busy_timeout(Duration::from_secs(3))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
        let mut store = Self {
            db,
            directory: directory.to_owned(),
            _lock: lock,
        };
        store.check_integrity()?;
        let version: usize = store
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > MIGRATIONS.len() {
            return Err(Error::Invalid(
                "Database schema is newer than this app; upgrade Recon before opening it.".into(),
            ));
        }
        if version == 0 {
            let tables: i64 = store.db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if tables != 0 {
                return Err(Error::Invalid(
                    "Unversioned nonempty database; refusing to guess its schema.".into(),
                ));
            }
        }
        if version > 0 && version < MIGRATIONS.len() {
            store.backup()?;
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = store
                .db
                .transaction_with_behavior(TransactionBehavior::Exclusive)?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", index + 1)?;
            tx.commit()?;
        }
        store.db.execute("INSERT OR IGNORE INTO source_accounts (provider, account_key) VALUES ('virgin', 'virgin-primary')", [])?;
        Ok(store)
    }

    fn check_integrity(&self) -> Result<()> {
        let check: String = self
            .db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok" || self.db.prepare("PRAGMA foreign_key_check")?.exists([])? {
            return Err(Error::Invalid(
                "Database integrity check failed; restore a verified backup before continuing."
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn backup(&self) -> Result<PathBuf> {
        let directory = self.directory.join("backups");
        config::private_directory(&directory)?;
        let suffix = crate::config::random_secret()?;
        let path = directory.join(format!("pending-{suffix}.sqlite3"));
        config::private_file(&path)?;
        let mut destination = Connection::open(&path)?;
        let backup = rusqlite::backup::Backup::new(&self.db, &mut destination)?;
        if backup.step(-1)? != rusqlite::backup::StepResult::Done {
            return Err(Error::Invalid("Backup could not acquire a database lock. Close external database tools and retry.".into()));
        }
        drop(backup);
        let check: String = destination.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok"
            || destination
                .prepare("PRAGMA foreign_key_check")?
                .exists([])?
        {
            return Err(Error::Invalid("Backup verification failed.".into()));
        }
        drop(destination);
        let verified = directory.join(format!("recon-{suffix}.sqlite3"));
        std::fs::rename(path, &verified)?;
        Ok(verified)
    }

    pub fn import(&mut self, filename: &str, bytes: &[u8]) -> Result<i64> {
        let hash = format!("{:x}", Sha256::digest(bytes));
        let source: i64 = self.db.query_row(
            "SELECT id FROM source_accounts WHERE account_key='virgin-primary'",
            [],
            |r| r.get(0),
        )?;
        if let Some(id) = self
            .db
            .query_row(
                "SELECT id FROM imports WHERE source_account_id=?1 AND sha256=?2",
                params![source, hash],
                |r| r.get(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        let observations = importer::parse(bytes)?;
        // Snapshot before first new import too; the backup includes schema and prior decisions.
        self.backup()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO imports (source_account_id, sha256, original_filename, original_bytes) VALUES (?1, ?2, ?3, ?4)", params![source, hash, filename, bytes])?;
        let id = tx.last_insert_rowid();
        for mut observation in observations {
            let mut purchase_id = None;
            if let Some(p) = observation.purchase.take() {
                let existing = tx.query_row(
                    "SELECT id, transaction_date, posting_date, amount_pence, merchant, card_used, original_currency FROM purchases WHERE source_account_id=?1 AND reference=?2",
                    params![source, p.reference], |r| Ok((r.get::<_, i64>(0)?, Purchase { reference: p.reference.clone(), transaction_date:r.get(1)?, posting_date:r.get(2)?, amount_pence:r.get(3)?, merchant:r.get(4)?, card_used:r.get(5)?, original_currency:r.get(6)? }))
                ).optional()?;
                if let Some((existing_id, existing_purchase)) = existing {
                    purchase_id = Some(existing_id);
                    if p != existing_purchase {
                        observation.disposition = "conflict";
                        observation.reason = Some(
                            "Reference matches an existing purchase but core fields differ; original purchase and decisions were preserved.",
                        );
                    }
                } else {
                    tx.execute("INSERT INTO purchases (source_account_id, reference, transaction_date, posting_date, amount_pence, currency, merchant, card_used, original_currency) VALUES (?1, ?2, ?3, ?4, ?5, 'GBP', ?6, ?7, ?8)", params![source, p.reference, p.transaction_date, p.posting_date, p.amount_pence, p.merchant, p.card_used, p.original_currency])?;
                    purchase_id = Some(tx.last_insert_rowid());
                }
            }
            tx.execute("INSERT INTO source_observations (import_id, source_account_id, record_number, source_text, disposition, reason, purchase_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![id, source, observation.record_number, observation.source_text, observation.disposition, observation.reason, purchase_id])?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn summary(&self, id: i64) -> Result<ImportSummary> {
        let filename = self
            .db
            .query_row(
                "SELECT original_filename FROM imports WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let mut counts = BTreeMap::new();
        let mut exceptions = Vec::new();
        let mut stmt = self.db.prepare("SELECT record_number, disposition, coalesce(reason, '') FROM source_observations WHERE import_id=?1 ORDER BY record_number")?;
        let records = stmt.query_map([id], |r| {
            Ok(Exception {
                record_number: r.get(0)?,
                disposition: r.get(1)?,
                reason: r.get(2)?,
            })
        })?;
        let mut total = 0;
        for row in records {
            let row = row?;
            *counts.entry(row.disposition.clone()).or_insert(0) += 1;
            total += 1;
            if row.disposition != "purchase" {
                exceptions.push(row);
            }
        }
        Ok(ImportSummary {
            id,
            filename,
            counts: counts.into_iter().collect(),
            exceptions,
            total,
        })
    }

    pub fn recent_imports(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .db
            .prepare("SELECT id, original_filename FROM imports ORDER BY id DESC LIMIT 20")?;
        Ok(stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn purchases(&self, page: i64) -> Result<PurchasePage> {
        let total: i64 = self
            .db
            .query_row("SELECT count(*) FROM purchases", [], |r| r.get(0))?;
        let pages = ((total + 49) / 50).max(1);
        let page = page.clamp(1, pages);
        let mut stmt = self.db.prepare(&format!(
            "{ROW_QUERY} ORDER BY p.transaction_date DESC, p.id DESC LIMIT 50 OFFSET ?1"
        ))?;
        let rows = stmt
            .query_map([(page - 1) * 50], map_purchase)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(PurchasePage {
            rows,
            page,
            pages,
            total,
        })
    }

    pub fn decide(&mut self, id: i64, expected: i64, account: &str) -> Result<PurchaseRow> {
        if !matches!(account, "" | "personal" | "joint") {
            return Err(Error::Invalid("Choose unset, Personal, or Joint.".into()));
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = tx
            .query_row(&format!("{ROW_QUERY} WHERE p.id=?1"), [id], map_purchase)
            .optional()?
            .ok_or(Error::NotFound)?;
        if current.version != expected {
            return Err(Error::Stale);
        }
        if current.conflict {
            return Err(Error::Invalid("This purchase has conflicting source observations; account edits are blocked pending resolution.".into()));
        }
        tx.execute(
            "INSERT INTO review_decisions (purchase_id, version, account) VALUES (?1, ?2, ?3)",
            params![
                id,
                expected + 1,
                if account.is_empty() {
                    None
                } else {
                    Some(account)
                }
            ],
        )?;
        let row = tx.query_row(&format!("{ROW_QUERY} WHERE p.id=?1"), [id], map_purchase)?;
        tx.commit()?;
        Ok(row)
    }
}

const ROW_QUERY: &str = "SELECT p.id, p.reference, p.transaction_date, p.amount_pence, p.merchant, p.card_used, coalesce(d.account, ''), coalesce(d.version, 0), EXISTS(SELECT 1 FROM source_observations o WHERE o.purchase_id=p.id AND o.disposition='conflict') FROM purchases p LEFT JOIN current_account_decisions d ON d.purchase_id=p.id";

fn map_purchase(r: &rusqlite::Row<'_>) -> rusqlite::Result<PurchaseRow> {
    let pennies: i64 = r.get(3)?;
    Ok(PurchaseRow {
        id: r.get(0)?,
        reference: r.get(1)?,
        transaction_date: r.get(2)?,
        amount: format!("{}.{:02}", pennies / 100, pennies % 100),
        merchant: r.get(4)?,
        card_used: r.get(5)?,
        account: r.get(6)?,
        version: r.get(7)?,
        conflict: r.get(8)?,
    })
}
