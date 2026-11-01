//! Match only account-scoped pot debits within recorded submission windows.
use super::*;
use chrono::{DateTime, FixedOffset};

pub(super) fn timestamp(value: &str) -> std::result::Result<DateTime<FixedOffset>, BankFailure> {
    DateTime::parse_from_rfc3339(value).map_err(|_| BankFailure::NoteMatchPending)
}

fn valid_pair(op: &Operation, tx: &Value) -> bool {
    let Ok(created) = tx["created"]
        .as_str()
        .ok_or(BankFailure::NoteMatchPending)
        .and_then(timestamp)
    else {
        return false;
    };
    let (Ok(start), Ok(end)) = (timestamp(&op.started_at), timestamp(&op.confirmed_at)) else {
        return false;
    };
    let marker = tx["metadata"]["recon_operation"]
        .as_str()
        .unwrap_or_default();
    created >= start - chrono::Duration::seconds(5)
        && created <= end + chrono::Duration::seconds(5)
        && tx["account_id"]
            .as_str()
            .is_none_or(|account| account == op.destination.account_id)
        && (marker.is_empty() || marker == op.dedupe_id)
        && [&tx["notes"], &tx["metadata"]["notes"]]
            .iter()
            .all(|value| {
                value
                    .as_str()
                    .is_none_or(|note| note.is_empty() || note == op.note || marker == op.dedupe_id)
            })
}

pub(super) fn match_transactions(
    operations: &[&Operation],
    transactions: &[Value],
    used: &BTreeSet<String>,
) -> std::result::Result<Vec<(i64, String)>, BankFailure> {
    let mut groups: BTreeMap<(&str, i64), Vec<&Operation>> = BTreeMap::new();
    for op in operations {
        groups
            .entry((&op.destination.pot_id, op.amount_pence))
            .or_default()
            .push(op);
    }
    let mut result = Vec::new();
    for ((pot, amount), mut ops) in groups {
        ops.sort_by(|a, b| a.started_at.cmp(&b.started_at).then(a.id.cmp(&b.id)));
        let mut candidates: Vec<_> = transactions
            .iter()
            .filter(|tx| {
                tx["currency"] == "GBP"
                    && tx["amount"].as_i64() == Some(-amount)
                    && tx["metadata"]["pot_id"] == pot
                    && tx["decline_reason"].as_str().is_none_or(str::is_empty)
                    && tx["id"].as_str().is_some_and(|id| {
                        crate::execution::valid_id(id, "tx_") && !used.contains(id)
                    })
                    && ops.iter().any(|op| valid_pair(op, tx))
            })
            .collect();
        candidates.sort_by_key(|tx| {
            (
                timestamp(tx["created"].as_str().unwrap()).unwrap(),
                tx["id"].as_str().unwrap(),
            )
        });
        if candidates.len() != ops.len() {
            return Err(BankFailure::NoteMatchPending);
        }
        // An existing recon marker takes priority over ordering on recovery.
        let mut remaining = Vec::new();
        for op in ops {
            let marked: Vec<_> = candidates
                .iter()
                .enumerate()
                .filter(|(_, tx)| tx["metadata"]["recon_operation"] == op.dedupe_id)
                .map(|(i, _)| i)
                .collect();
            if marked.len() > 1 {
                return Err(BankFailure::NoteMatchPending);
            }
            if let Some(&index) = marked.first() {
                let tx = candidates.remove(index);
                if !valid_pair(op, tx) {
                    return Err(BankFailure::NoteMatchPending);
                }
                result.push((op.id, tx["id"].as_str().unwrap().into()));
            } else {
                remaining.push(op);
            }
        }
        // Deposits are sequential. Equal-amount transfers are assigned one-to-one
        // by creation time, never by an arbitrary first result from the API.
        for (index, (op, tx)) in remaining.iter().zip(&candidates).enumerate() {
            if !valid_pair(op, tx) {
                return Err(BankFailure::NoteMatchPending);
            }
            if index > 0
                && timestamp(tx["created"].as_str().unwrap())?
                    == timestamp(candidates[index - 1]["created"].as_str().unwrap())?
                && op.note != remaining[index - 1].note
            {
                return Err(BankFailure::NoteMatchPending);
            }
            result.push((op.id, tx["id"].as_str().unwrap().into()));
        }
    }
    Ok(result)
}
