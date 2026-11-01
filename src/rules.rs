//! Explicitly approved account defaults. Purchase overrides always win.
use crate::{
    Error, Result,
    review::{DetailRow, normalize},
    storage::Store,
};
use rusqlite::{Transaction, params};

#[derive(Clone, Debug)]
pub struct AccountRule {
    pub id: i64,
    pub merchant: String,
    pub key: String,
    pub card: String,
    pub version: i64,
    pub account: String,
}

impl Store {
    pub fn account_rules(&self) -> Result<Vec<AccountRule>> {
        Ok(self.db.prepare(
            "SELECT id,merchant_name,merchant_key,card,version,coalesce(account,'') FROM current_account_rules ORDER BY merchant_key,card"
        )?.query_map([], |r| Ok(AccountRule {
            id: r.get(0)?, merchant: r.get(1)?, key: r.get(2)?, card: r.get(3)?,
            version: r.get(4)?, account: r.get(5)?,
        }))?.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn enrich_accounts(&self, rows: &mut [DetailRow]) -> Result<()> {
        let rules = self.account_rules()?;
        let aliases = self.aliases()?;
        for row in rows {
            if !row.account.is_empty() {
                row.account_source = format!("purchase choice v{}", row.version);
                continue;
            }
            let key = normalize(&row.merchant);
            let key = aliases.get(&key).unwrap_or(&key);
            let rule = rules
                .iter()
                .filter(|r| {
                    &r.key == key
                        && !r.account.is_empty()
                        && (r.card.is_empty() || r.card == row.card)
                })
                .max_by_key(|r| !r.card.is_empty());
            if let Some(rule) = rule {
                row.account = rule.account.clone();
                row.account_source = format!("account rule #{} v{}", rule.id, rule.version);
            } else {
                row.account_source = "missing".into();
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_rule(merchant: &str, card: &str, account: &str) -> Result<()> {
    if normalize(merchant).is_empty()
        || merchant.len() > 1000
        || card.len() > 200
        || !matches!(account, "" | "personal" | "joint")
    {
        return Err(Error::Invalid(
            "Choose an exact merchant, optional card, and Personal/Joint (or disable).".into(),
        ));
    }
    Ok(())
}

pub(crate) fn save_rule(
    tx: &Transaction<'_>,
    merchant: &str,
    card: &str,
    account: &str,
) -> Result<()> {
    validate_rule(merchant, card, account)?;
    tx.execute(
        "INSERT INTO account_rules(merchant_key,merchant_name,card,version,account)
         SELECT ?1,?2,?3,coalesce(max(version),0)+1,?4 FROM account_rules WHERE merchant_key=?1 AND card=?3",
        params![normalize(merchant),merchant.trim(),card.trim(), if account.is_empty() {None} else {Some(account)}],
    )?;
    Ok(())
}
