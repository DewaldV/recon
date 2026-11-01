use rusqlite::{Connection, params};

const MIGRATION: &str = include_str!("../migrations/0001_initial.sql");

fn database() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    let tx = db.transaction().unwrap();
    tx.execute_batch(MIGRATION).unwrap();
    tx.pragma_update(None, "user_version", 1).unwrap();
    tx.commit().unwrap();
    db.execute_batch(
        "INSERT INTO source_accounts VALUES (1, 'virgin', 'synthetic-one');
         INSERT INTO source_accounts VALUES (2, 'virgin', 'synthetic-two');",
    )
    .unwrap();
    db
}

fn purchase(
    db: &Connection,
    id: i64,
    source: i64,
    reference: &str,
    amount: i64,
) -> rusqlite::Result<usize> {
    db.execute(
        "INSERT INTO purchases VALUES (?1, ?2, ?3, '2026-10-01', '2026-10-02', ?4,
         'GBP', 'SYNTHETIC SHOP', '0000')",
        params![id, source, reference, amount],
    )
}

fn import(db: &Connection, id: i64, source: i64) -> rusqlite::Result<usize> {
    db.execute(
        "INSERT INTO imports (id, source_account_id, sha256, original_filename, original_bytes)
         VALUES (?1, ?2, ?3, 'synthetic.csv', ?4)",
        params![id, source, "a".repeat(64), b"synthetic file".as_slice()],
    )
}

#[test]
fn migration_applies_and_enforces_source_scoped_identity() {
    let db = database();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    purchase(&db, 1, 1, "000001", 1234).unwrap();
    assert!(purchase(&db, 2, 1, "000001", 1234).is_err());
    purchase(&db, 2, 2, "000001", 1234).unwrap();
    purchase(&db, 3, 1, "000002", 1234).unwrap();
    assert!(purchase(&db, 4, 1, "null", 1234).is_err());
    assert!(purchase(&db, 4, 1, "", 1234).is_err());
    assert!(purchase(&db, 4, 1, "000004", -1).is_err());
    assert!(purchase(&db, 4, 999, "000004", 1234).is_err());
    let reference: String = db
        .query_row("SELECT reference FROM purchases WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(reference, "000001");
}

#[test]
fn imports_are_unique_per_source_and_immutable() {
    let db = database();
    import(&db, 1, 1).unwrap();
    assert!(import(&db, 2, 1).is_err());
    import(&db, 2, 2).unwrap();
    assert!(
        db.execute("UPDATE imports SET original_filename = 'changed'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM imports", []).is_err());
}

#[test]
fn observation_links_cannot_cross_accounts_or_omit_exception_reasons() {
    let db = database();
    import(&db, 1, 1).unwrap();
    purchase(&db, 1, 1, "000001", 1234).unwrap();
    purchase(&db, 2, 2, "000002", 1234).unwrap();
    let insert = "INSERT INTO source_observations
        (import_id, source_account_id, record_number, source_text, disposition, reason, purchase_id)
        VALUES (1, ?1, ?2, 'synthetic record', ?3, ?4, ?5)";
    db.execute(insert, params![1, 2, "purchase", None::<String>, 1])
        .unwrap();
    assert!(
        db.execute(insert, params![1, 3, "purchase", None::<String>, 2])
            .is_err()
    );
    assert!(
        db.execute(insert, params![2, 3, "purchase", None::<String>, 2])
            .is_err()
    );
    assert!(
        db.execute(
            insert,
            params![1, 3, "missing_reference", None::<String>, None::<i64>]
        )
        .is_err()
    );
    db.execute(
        insert,
        params![1, 3, "missing_reference", "No reference", None::<i64>],
    )
    .unwrap();
    assert!(
        db.execute("UPDATE source_observations SET source_text = 'changed'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM source_observations", []).is_err());
}

#[test]
fn decisions_are_versioned_append_only_and_can_be_cleared() {
    let db = database();
    purchase(&db, 1, 1, "000001", 1234).unwrap();
    let insert = "INSERT INTO review_decisions (purchase_id, version, account) VALUES (1, ?1, ?2)";
    assert!(db.execute(insert, params![2, "joint"]).is_err());
    db.execute(insert, params![1, "personal"]).unwrap();
    assert!(db.execute(insert, params![1, "joint"]).is_err());
    assert!(db.execute(insert, params![2, "invalid"]).is_err());
    db.execute(insert, params![2, "joint"]).unwrap();
    let account: String = db
        .query_row("SELECT account FROM current_account_decisions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(account, "joint");
    db.execute(insert, params![3, None::<String>]).unwrap();
    let account: Option<String> = db
        .query_row("SELECT account FROM current_account_decisions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(account, None);
    assert!(
        db.execute("UPDATE review_decisions SET account = 'personal'", [])
            .is_err()
    );
    assert!(db.execute("DELETE FROM review_decisions", []).is_err());
    assert!(
        db.execute("UPDATE purchases SET amount_pence = 999", [])
            .is_err()
    );
}

#[test]
fn failed_transaction_leaves_no_partial_schema_or_version() {
    let mut db = Connection::open_in_memory().unwrap();
    {
        let tx = db.transaction().unwrap();
        tx.execute_batch(MIGRATION).unwrap();
        assert!(tx.execute_batch("THIS IS INVALID SQL;").is_err());
        // Drop rolls back, just as the migration runner must on any error.
    }
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0);
}
