//! Explicitly approved account defaults. Purchase overrides always win.
use crate::{
    Error, Result,
    review::{DetailRow, normalize},
    storage::Store,
};
use rusqlite::{Transaction, params};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct AccountRule {
    pub id: i64,
    pub merchant: String,
    pub key: String,
    pub card: String,
    pub version: i64,
    pub account: String,
}

pub struct MerchantChoice {
    pub key: String,
    pub name: String,
    pub aliases: String,
    pub cards: BTreeSet<String>,
}
impl MerchantChoice {
    pub fn cards_json(&self) -> String {
        serde_json::to_string(&self.cards).expect("card choices contain only strings")
    }
}

pub struct RuleChoices {
    pub merchants: Vec<MerchantChoice>,
    pub cards: Vec<String>,
}

pub struct RulePurchaseImpact {
    pub id: i64,
    pub merchant: String,
    pub before: String,
    pub after: String,
    pub effect: &'static str,
    pub restage: bool,
}

#[derive(Default)]
pub struct RuleImpact {
    pub purchases: Vec<RulePurchaseImpact>,
    pub changed: usize,
    pub explicit_choices: usize,
    pub fixed_runs: usize,
    pub restage: usize,
}

fn matching_rule<'a>(rules: &'a [AccountRule], key: &str, card: &str) -> Option<&'a AccountRule> {
    rules
        .iter()
        .filter(|r| r.key == key && !r.account.is_empty() && (r.card.is_empty() || r.card == card))
        .max_by_key(|r| !r.card.is_empty())
}

impl Store {
    /// Canonical choices include catalogue-only merchants and existing rules.
    /// Aliases are searchable labels, but selecting one always targets its canonical merchant.
    pub fn rule_choices(&self) -> Result<RuleChoices> {
        let aliases = self.aliases()?;
        let names = self
            .db
            .prepare(
                "SELECT original_name AS name,0 AS priority FROM current_merchants
             UNION SELECT merchant_name,1 FROM current_account_rules
             UNION SELECT merchant,2 FROM purchases
             UNION SELECT original_name,3 FROM merchant_aliases ORDER BY priority,name",
            )?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut merchants: BTreeMap<String, MerchantChoice> = BTreeMap::new();
        let mut variants: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for name in names {
            let key = normalize(&name);
            let target = aliases.get(&key).unwrap_or(&key).clone();
            if target != key {
                variants.entry(target).or_default().insert(name);
            } else {
                merchants.entry(key.clone()).or_insert(MerchantChoice {
                    key,
                    name,
                    aliases: String::new(),
                    cards: BTreeSet::new(),
                });
            }
        }
        for (key, names) in variants {
            if let Some(merchant) = merchants.get_mut(&key) {
                merchant.aliases = names.into_iter().collect::<Vec<_>>().join(", ");
            }
        }
        let mut cards = BTreeSet::new();
        let mut stmt = self
            .db
            .prepare("SELECT DISTINCT merchant,card_used FROM purchases WHERE card_used!=''")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (merchant, card) = row?;
            let key = normalize(&merchant);
            if let Some(merchant) = merchants.get_mut(aliases.get(&key).unwrap_or(&key)) {
                merchant.cards.insert(card.clone());
            }
            cards.insert(card);
        }
        // Keep previously configured cards editable, even without a matching purchase.
        cards.extend(
            self.account_rules()?
                .into_iter()
                .map(|r| r.card)
                .filter(|c| !c.is_empty()),
        );
        Ok(RuleChoices {
            merchants: merchants.into_values().collect(),
            cards: cards.into_iter().collect(),
        })
    }

    pub fn canonical_rule_merchant(&self, merchant: &str) -> Result<String> {
        let key = normalize(merchant);
        let aliases = self.aliases()?;
        let key = aliases.get(&key).unwrap_or(&key);
        self.rule_choices()?
            .merchants
            .into_iter()
            .find(|m| &m.key == key)
            .map(|m| m.name)
            .ok_or_else(|| {
                Error::Invalid(
                    "Choose a merchant from your purchases or merchant catalogue.".into(),
                )
            })
    }

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
            let rule = matching_rule(&rules, key, &row.card);
            if let Some(rule) = rule {
                row.account = rule.account.clone();
                row.account_source = format!("account rule #{} v{}", rule.id, rule.version);
            } else {
                row.account_source = "missing".into();
            }
        }
        Ok(())
    }

    pub(crate) fn rule_impact(
        &self,
        rows: &[DetailRow],
        merchant: &str,
        card: &str,
        account: &str,
    ) -> Result<RuleImpact> {
        let key = normalize(merchant);
        let card = card.trim();
        let mut rules = self.account_rules()?;
        let version = rules
            .iter()
            .find(|r| r.key == key && r.card == card)
            .map_or(1, |r| r.version + 1);
        // Approval is revision-checked; this is the ID save_rule will insert.
        let id = self
            .db
            .query_row("SELECT coalesce(max(id),0)+1 FROM account_rules", [], |r| {
                r.get(0)
            })?;
        rules.retain(|r| r.key != key || r.card != card);
        rules.push(AccountRule {
            id,
            key: key.clone(),
            merchant: merchant.into(),
            card: card.into(),
            version,
            account: account.into(),
        });
        let mut impact = RuleImpact::default();
        for row in rows {
            let mut after = row.clone();
            let effect = if row.in_run {
                impact.fixed_runs += 1;
                "Recorded run preserved"
            } else if row.account_source.starts_with("purchase choice") {
                impact.explicit_choices += 1;
                "Explicit purchase choice preserved"
            } else {
                if let Some(rule) = matching_rule(&rules, &key, &row.card) {
                    after.account = rule.account.clone();
                    after.account_source = format!("account rule #{} v{}", rule.id, rule.version);
                } else {
                    after.account.clear();
                    after.account_source = "missing".into();
                }
                if after.signature() != row.signature() {
                    impact.changed += 1;
                    "Effective account decision changes"
                } else {
                    "Existing decision preserved"
                }
            };
            let restage = row.staged && after.signature() != row.signature();
            impact.restage += usize::from(restage);
            impact.purchases.push(RulePurchaseImpact {
                id: row.id,
                merchant: row.merchant.clone(),
                before: row.account.clone(),
                after: after.account,
                effect,
                restage,
            });
        }
        Ok(impact)
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
