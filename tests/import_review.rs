use std::{fs, os::unix::fs::PermissionsExt};

use recon::{
    Error,
    importer::{self, pennies},
    storage::Store,
};
use rusqlite::Connection;

const FIRST: &[u8] = include_bytes!("fixtures/virgin-first.csv");
const OVERLAP: &[u8] = include_bytes!("fixtures/virgin-overlap.csv");

fn changed(from: &str, to: &str) -> Vec<u8> {
    String::from_utf8(FIRST.to_vec())
        .unwrap()
        .replace(from, to)
        .into_bytes()
}

#[test]
fn strict_money_parser() {
    for (text, expected) in [
        ("0.01", 1),
        ("12.34", 1234),
        ("1,234.56", 123456),
        ("1234.56", 123456),
    ] {
        assert_eq!(pennies(text), Some(expected));
    }
    for text in [
        "",
        "12",
        "12.3",
        "12.345",
        "-1.00",
        "+1.00",
        " 1.00",
        "0.00",
        ".01",
        "1,23.45",
        "1234,567.89",
        "1.2.3",
        "99999999999999999999.00",
        "£1.00",
    ] {
        assert_eq!(pennies(text), None, "{text}");
    }
}

#[test]
fn import_overlap_restart_and_stale_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let first = store.import("first.csv", FIRST).unwrap();
    assert_eq!(store.summary(first).unwrap().total, 3);
    let page = store.purchases(1).unwrap();
    assert_eq!(page.total, 2); // Identical-looking purchases have distinct references.
    let id = page.rows[0].id;
    store.decide(id, 0, "personal").unwrap();
    assert!(matches!(store.decide(id, 0, "joint"), Err(Error::Stale)));
    assert!(store.decide(id, 1, "invalid").is_err());
    assert_eq!(store.import("renamed.csv", FIRST).unwrap(), first);
    store.import("overlap.csv", OVERLAP).unwrap();
    assert_eq!(store.purchases(1).unwrap().total, 3);
    assert_eq!(store.recent_imports().unwrap().len(), 2);
    drop(store);
    let mut store = Store::open(directory.path()).unwrap();
    let page = store.purchases(1).unwrap();
    let row = page.rows.iter().find(|p| p.id == id).unwrap();
    assert_eq!(row.account, "personal");
    assert_eq!(row.version, 1);
    store.decide(id, 1, "").unwrap();
    assert!(
        store
            .purchases(1)
            .unwrap()
            .rows
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .account
            .is_empty()
    );
    assert_eq!(
        fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(directory.path().join("recon.sqlite3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn raw_archive_observation_counts_and_backups_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let id = store.import("first.csv", FIRST).unwrap();
    store.import("overlap.csv", OVERLAP).unwrap();
    let backup = store.backup().unwrap();
    let restored = tempfile::tempdir().unwrap();
    fs::copy(backup, restored.path().join("recon.sqlite3")).unwrap();
    let restored_store = Store::open(restored.path()).unwrap();
    assert_eq!(restored_store.purchases(1).unwrap().total, 3);
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    let bytes: Vec<u8> = db
        .query_row(
            "SELECT original_bytes FROM imports WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bytes, FIRST);
    let count: i64 = db
        .query_row("SELECT count(*) FROM source_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 5);
    assert_eq!(
        store.summary(id).unwrap().exceptions[0].disposition,
        "missing_reference"
    );
    assert!(Store::open(dir.path()).is_err()); // Exclusive app ownership.
}

#[test]
fn conflicts_do_not_overwrite_purchase_or_decision() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("first.csv", FIRST).unwrap();
    let row_id = store.purchases(1).unwrap().rows[0].id;
    store.decide(row_id, 0, "joint").unwrap();
    let id = store
        .import("changed.csv", &changed("12.34", "99.00"))
        .unwrap();
    assert_eq!(
        store
            .summary(id)
            .unwrap()
            .counts
            .iter()
            .find(|(kind, _)| kind == "conflict")
            .unwrap()
            .1,
        2
    );
    let page = store.purchases(1).unwrap();
    assert_eq!(page.total, 2);
    assert!(page.rows.iter().all(|p| p.amount == "12.34" && p.conflict));
    assert_eq!(
        page.rows.iter().find(|p| p.id == row_id).unwrap().account,
        "joint"
    );
    assert!(store.decide(row_id, 1, "personal").is_err());
}

#[test]
fn every_duplicate_occurrence_is_quarantined() {
    let bytes = changed("00000000000000000000002", "00000000000000000000001");
    let observations = importer::parse(&bytes).unwrap();
    assert_eq!(observations.len(), 3);
    assert_eq!(
        observations
            .iter()
            .filter(|r| r.disposition == "duplicate_reference")
            .count(),
        2
    );
    assert!(observations.iter().all(|r| r.purchase.is_none()));
}

#[test]
fn classification_and_validation_are_explicit() {
    for (from, to, expected) in [
        ("12.34", "12.3", "malformed"),
        ("2026-10-01", "2026-02-30", "malformed"),
        ("2026-10-02,12.34", "null,12.34", "malformed"),
        ("DBIT,PR,BILLED", "DBIT,PR,PENDING", "pending"),
        ("DBIT,PR,BILLED", "CRDT,PR,BILLED", "credit"),
        ("DBIT,PR,BILLED", "DBIT,FE,BILLED", "fee"),
        ("DBIT,PR,BILLED", "CRDT,PY,BILLED", "payment"),
        ("BILLED,GBP", "BILLED,INVALID", "unsupported"),
        ("SYNTHETIC SHOP", "SYNTHETIC, SHOP", "purchase"),
    ] {
        let observations = importer::parse(&changed(from, to)).unwrap();
        assert_eq!(observations[0].disposition, expected, "{from} → {to}");
        assert_eq!(observations.len(), 3);
    }
    let foreign = importer::parse(&changed("BILLED,GBP", "BILLED,USD")).unwrap();
    assert_eq!(foreign[0].purchase.as_ref().unwrap().amount_pence, 1234);
    assert_eq!(
        foreign[0].purchase.as_ref().unwrap().original_currency,
        "USD"
    );
    let quoted = importer::parse(OVERLAP).unwrap();
    assert_eq!(
        quoted[1].purchase.as_ref().unwrap().merchant,
        "SYNTHETIC CAFE, TEST"
    );
    let mut bom = b"\xef\xbb\xbf".to_vec();
    bom.extend_from_slice(FIRST);
    assert_eq!(importer::parse(&bom).unwrap().len(), 3);
}

#[test]
fn fatal_import_has_no_partial_records() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    assert!(store.import("bad.csv", b"wrong,headers\n1,2").is_err());
    assert!(store.import("binary.csv", &[0xff, 0xfe]).is_err());
    assert!(
        store
            .import("big.csv", &vec![b'x'; importer::MAX_UPLOAD + 1])
            .is_err()
    );
    assert_eq!(store.recent_imports().unwrap().len(), 0);
    assert_eq!(store.purchases(1).unwrap().total, 0);
}

#[test]
fn newer_unknown_and_old_schemas_are_handled_safely() {
    let newer = tempfile::tempdir().unwrap();
    Connection::open(newer.path().join("recon.sqlite3"))
        .unwrap()
        .execute_batch("PRAGMA user_version=999;")
        .unwrap();
    assert!(Store::open(newer.path()).is_err());
    let unknown = tempfile::tempdir().unwrap();
    Connection::open(unknown.path().join("recon.sqlite3"))
        .unwrap()
        .execute_batch("CREATE TABLE unknown (id INTEGER);")
        .unwrap();
    assert!(Store::open(unknown.path()).is_err());
    let old = tempfile::tempdir().unwrap();
    let db = Connection::open(old.path().join("recon.sqlite3")).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch("PRAGMA user_version=1;").unwrap();
    drop(db);
    let _store = Store::open(old.path()).unwrap();
    assert_eq!(fs::read_dir(old.path().join("backups")).unwrap().count(), 1);
}

#[test]
fn symlinked_database_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("target"), b"").unwrap();
    std::os::unix::fs::symlink(
        other.path().join("target"),
        dir.path().join("recon.sqlite3"),
    )
    .unwrap();
    assert!(Store::open(dir.path()).is_err());
}

#[test]
fn storage_failure_rolls_back_the_entire_import() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER synthetic_failure BEFORE INSERT ON source_observations WHEN NEW.record_number=3 BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    assert!(store.import("first.csv", FIRST).is_err());
    assert!(store.recent_imports().unwrap().is_empty());
    assert_eq!(store.purchases(1).unwrap().total, 0);
    let count: i64 = db
        .query_row("SELECT count(*) FROM source_observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn failed_production_migration_preserves_previous_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(
        "ALTER TABLE purchases ADD COLUMN original_currency TEXT; PRAGMA user_version=1;",
    )
    .unwrap();
    assert!(Store::open(dir.path()).is_err());
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
}
