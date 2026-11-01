//! Pure, bounded Virgin parsing. Original file bytes are archived by storage.
use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;

use crate::{Error, Result};

pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
const HEADERS: [&str; 14] = [
    "Transaction Date",
    "Posting Date",
    "Billing Amount",
    "Merchant",
    "Merchant City",
    "Merchant State",
    "Merchant Postcode",
    "Reference Number",
    "Debit or Credit",
    "SICMCC Code",
    "Status",
    "Transaction Currency",
    "Additional Card Holder",
    "Card Used",
];

pub const REPAIR_METHOD: &str = "monzo-vcc-extra-columns-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Purchase {
    pub reference: String,
    pub transaction_date: String,
    pub posting_date: String,
    pub amount_pence: i64,
    pub merchant: String,
    pub card_used: String,
    pub original_currency: String,
}

#[derive(Debug)]
pub struct Observation {
    pub record_number: i64,
    pub source_text: String,
    pub disposition: &'static str,
    pub reason: Option<&'static str>,
    pub purchase: Option<Purchase>,
    pub posting_date: Option<String>,
    /// Canonical columns after the CLI-compatible extra-column repair.
    pub repaired: Option<Vec<String>>,
}

/// Positive decimal GBP with exactly two decimal places; optional valid grouping.
pub fn pennies(text: &str) -> Option<i64> {
    let (whole, fractional) = text.split_once('.')?;
    if fractional.len() != 2 || !fractional.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let groups: Vec<_> = whole.split(',').collect();
    if groups
        .iter()
        .any(|g| g.is_empty() || !g.bytes().all(|b| b.is_ascii_digit()))
        || (groups.len() > 1 && (groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3)))
    {
        return None;
    }
    let whole: i64 = groups.concat().parse().ok()?;
    let result = whole
        .checked_mul(100)?
        .checked_add(fractional.parse().ok()?)?;
    (result > 0).then_some(result)
}

pub(crate) fn date(text: &str) -> bool {
    text.len() == 10
        && NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == text)
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Observation>> {
    if bytes.len() > MAX_UPLOAD {
        return Err(Error::Invalid("CSV exceeds the 10 MiB limit.".into()));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid("CSV must use UTF-8 encoding.".into()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes());
    let header = reader
        .headers()
        .map_err(|_| Error::Invalid("Cannot read CSV headers.".into()))?
        .clone();
    let actual: HashSet<_> = header.iter().collect();
    if header.len() != HEADERS.len()
        || actual.len() != HEADERS.len()
        || HEADERS.iter().any(|h| !actual.contains(h))
    {
        return Err(Error::Invalid("Expected the 14 original Virgin export columns, including Reference Number. Do not upload a spreadsheet batch CSV.".into()));
    }
    let indices: Vec<_> = HEADERS
        .iter()
        .map(|h| header.iter().position(|x| x == *h).unwrap())
        .collect();
    let mut observations = Vec::new();
    let mut record = csv::StringRecord::new();
    loop {
        let start = reader.position().byte() as usize;
        match reader.read_record(&mut record) {
            Ok(false) => break,
            Err(_) => {
                return Err(Error::Invalid(
                    "CSV contains unreadable records; import rejected without changes.".into(),
                ));
            }
            Ok(true) => {}
        }
        let end = reader.position().byte() as usize;
        let mut row = Observation {
            record_number: observations.len() as i64 + 2,
            source_text: text[start..end].to_owned(),
            disposition: "malformed",
            reason: Some("Unexpected column count; no validated automatic repair was available."),
            purchase: None,
            posting_date: None,
            repaired: None,
        };
        if record.len() == HEADERS.len() {
            let values: Vec<_> = indices.iter().map(|&i| &record[i]).collect();
            row.posting_date = date(values[1]).then(|| values[1].to_owned());
            classify(&values, &mut row);
        } else if header.iter().eq(HEADERS)
            && let Some(fields) = repair_extra_columns(&record)
        {
            let values: Vec<_> = fields.iter().map(String::as_str).collect();
            row.posting_date = date(values[1]).then(|| values[1].to_owned());
            classify(&values, &mut row);
            if row.disposition != "malformed" && row.disposition != "unsupported" {
                row.repaired = Some(fields);
            }
        }
        observations.push(row);
    }
    // Quarantine every occurrence, not just the second one, of a repeated identity.
    let mut counts = HashMap::new();
    for row in &observations {
        if let Some(p) = &row.purchase {
            *counts.entry(p.reference.clone()).or_insert(0) += 1;
        }
    }
    for row in &mut observations {
        if row
            .purchase
            .as_ref()
            .is_some_and(|p| counts[&p.reference] > 1)
        {
            row.disposition = "duplicate_reference";
            row.reason = Some("Repeated purchase reference in this file; identity needs review.");
            row.purchase = None;
        }
    }
    Ok(observations)
}

/// Like monzo-cli, fold surplus middle fields into Merchant, retaining the final
/// ten columns. This intentionally preserves its merchant naming convention,
/// including city fragments. Only the standard layout with validated anchors
/// qualifies; already-correct CSV quoting is handled by the CSV reader above.
fn repair_extra_columns(record: &csv::StringRecord) -> Option<Vec<String>> {
    if !(15..=32).contains(&record.len()) {
        return None;
    }
    let tail = record.len() - 10;
    let mut fields: Vec<String> = record.iter().take(3).map(str::to_owned).collect();
    fields.push(
        record
            .iter()
            .skip(3)
            .take(tail - 3)
            .collect::<Vec<_>>()
            .join(" "),
    );
    fields.extend(record.iter().skip(tail).map(str::to_owned));
    let country = &fields[5];
    let reference = &fields[7];
    let card = &fields[13];
    if country.len() != 3
        || !country.bytes().all(|c| c.is_ascii_uppercase())
        || reference.is_empty()
        || !reference.bytes().all(|c| c.is_ascii_digit())
        || card.len() != 4
        || !card.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(fields)
}

pub(crate) fn repaired_csv(records: &[Vec<String>]) -> Result<Vec<u8>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(HEADERS).map_err(|_| Error::Worker)?;
    for record in records {
        writer.write_record(record).map_err(|_| Error::Worker)?;
    }
    writer.into_inner().map_err(|_| Error::Worker)
}

fn classify(v: &[&str], row: &mut Observation) {
    row.reason = Some("Invalid date, amount, or export field value.");
    let posting = v[1].is_empty() || v[1] == "null";
    if !date(v[0])
        || (!posting && !date(v[1]))
        || pennies(v[2]).is_none()
        || !matches!(v[8], "DBIT" | "CRDT")
        || !matches!(v[9], "PR" | "PY" | "FE" | "" | "null")
        || !matches!(v[10], "BILLED" | "PENDING")
        || !matches!(v[12], "true" | "false")
        || v[3].trim().is_empty()
    {
        return;
    }
    if v[11].len() != 3 || !v[11].bytes().all(|b| b.is_ascii_uppercase()) {
        row.disposition = "unsupported";
        row.reason = Some("Unsupported original transaction currency code.");
        return;
    }
    let exception = if v[10] == "PENDING" {
        Some(("pending", "Pending transaction; not a billed purchase."))
    } else if v[9] == "PY" {
        Some(("payment", "Credit-card payment; not a purchase."))
    } else if v[9] == "FE" {
        Some(("fee", "Fee requires a later handling decision."))
    } else if v[8] == "CRDT" {
        Some((
            "credit",
            "Credit/refund requires a later handling decision.",
        ))
    } else if v[9] != "PR" {
        Some(("unsupported", "Unrecognised purchase classification."))
    } else if posting {
        Some(("malformed", "Billed purchase has no posting date."))
    } else if v[7].trim().is_empty() || v[7].trim().eq_ignore_ascii_case("null") {
        Some((
            "missing_reference",
            "Purchase has no reference; identity needs review.",
        ))
    } else {
        None
    };
    if let Some((kind, reason)) = exception {
        row.disposition = kind;
        row.reason = Some(reason);
        return;
    }
    row.disposition = "purchase";
    row.reason = None;
    row.purchase = Some(Purchase {
        reference: v[7].trim().to_owned(),
        transaction_date: v[0].into(),
        posting_date: v[1].into(),
        amount_pence: pennies(v[2]).unwrap(),
        merchant: v[3].into(),
        card_used: v[13].into(),
        original_currency: v[11].into(),
    });
}
