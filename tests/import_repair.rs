use recon::{
    importer,
    review::{Change, Filters},
    storage::Store,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn csv(rows: &[&str]) -> Vec<u8> {
    format!(
        "{}\n{}\n",
        include_str!("fixtures/virgin-first.csv")
            .lines()
            .next()
            .unwrap(),
        rows.join("\n")
    )
    .into_bytes()
}
const BAD: &str = "2026-10-03,2026-10-04,4.01,SYNTHETIC RAIL, TOWN,TEST CITY,DEU,04109,12345678900000000000001,DBIT,PR,BILLED,GBP,false,0000";
const CITY: &str = "2026-09-13,2026-09-14,19.99,SYNTHETIC SUBSCRIPTION,LONDON, ENGLA,GBR,WC1V7HP,12345678900000000000002,DBIT,PR,BILLED,GBP,false,0000";

// Seed the exact archive shape produced by the previous parser, without relying
// on the new parser to simulate its own migration input.
fn legacy(dir: &tempfile::TempDir, bytes: &[u8]) -> i64 {
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute("INSERT INTO imports(source_account_id,sha256,original_filename,original_bytes) VALUES(1,?1,'legacy.csv',?2)",params![format!("{:x}",Sha256::digest(bytes)),bytes]).unwrap();
    let id = db.last_insert_rowid();
    for (i, line) in std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .skip(1)
        .enumerate()
    {
        db.execute("INSERT INTO source_observations(import_id,source_account_id,record_number,source_text,disposition,reason) VALUES(?1,1,?2,?3,'malformed','Unexpected column count; no automatic repair was attempted.')",params![id,i as i64+2,format!("{line}\n")]).unwrap();
    }
    id
}

#[test]
fn cli_compatible_repairs_preserve_financial_fields_and_source_and_quoted_csv() {
    let bytes = csv(&[BAD, CITY]);
    let rows = importer::parse(&bytes).unwrap();
    assert_eq!(rows[0].source_text, format!("{BAD}\n"));
    let p = rows[0].purchase.as_ref().unwrap();
    assert_eq!(p.merchant, "SYNTHETIC RAIL  TOWN");
    assert_eq!(
        (
            p.amount_pence,
            p.reference.as_str(),
            p.card_used.as_str(),
            p.posting_date.as_str()
        ),
        (401, "12345678900000000000001", "0000", "2026-10-04")
    );
    assert!(rows.iter().all(|r| r.repaired.is_some()));
    assert_eq!(
        rows[1].purchase.as_ref().unwrap().merchant,
        "SYNTHETIC SUBSCRIPTION LONDON"
    );
    let quoted = BAD.replace("SYNTHETIC RAIL, TOWN", "\"SYNTHETIC RAIL, TOWN\"");
    let rows = importer::parse(&csv(&[&quoted])).unwrap();
    assert!(rows[0].repaired.is_none());
    assert_eq!(
        rows[0].purchase.as_ref().unwrap().merchant,
        "SYNTHETIC RAIL, TOWN"
    );
    let multiple = BAD.replace("SYNTHETIC RAIL, TOWN", "SYNTHETIC, RAIL, TOWN");
    assert_eq!(
        importer::parse(&csv(&[&multiple])).unwrap()[0]
            .purchase
            .as_ref()
            .unwrap()
            .merchant,
        "SYNTHETIC  RAIL  TOWN"
    );
}

#[test]
fn malformed_anchors_and_reordered_malformed_headers_are_not_guessed() {
    for bad in [
        BAD.replace("4.01", "4.0"),
        BAD.replace("2026-10-04", "2026-02-30"),
        BAD.replace("DBIT,PR,BILLED", "WRONG,PR,BILLED"),
        BAD.replace("DEU,04109", "INVALID,04109"),
        BAD.replace("false,0000", "false,0,000"),
        BAD.replace("12345678900000000000001", "123,456"),
        BAD.replace("4.01", "1,234.56"),
    ] {
        let rows = importer::parse(&csv(&[&bad])).unwrap();
        assert!(rows[0].purchase.is_none(), "{bad}");
        assert!(rows[0].repaired.is_none(), "{bad}");
    }
    let swapped = String::from_utf8(csv(&[BAD])).unwrap().replace(
        "Transaction Date,Posting Date",
        "Posting Date,Transaction Date",
    );
    assert!(
        importer::parse(swapped.as_bytes()).unwrap()[0]
            .repaired
            .is_none()
    );
}

#[test]
fn repaired_duplicates_and_nonpurchases_keep_their_classification() {
    let rows = importer::parse(&csv(&[BAD, BAD])).unwrap();
    assert!(
        rows.iter()
            .all(|r| r.disposition == "duplicate_reference" && r.purchase.is_none())
    );
    for (value, expected) in [
        ("CRDT,PR,BILLED", "credit"),
        ("DBIT,FE,BILLED", "fee"),
        ("CRDT,PY,BILLED", "payment"),
        ("DBIT,PR,PENDING", "pending"),
    ] {
        let changed = BAD.replace("DBIT,PR,BILLED", value);
        let rows = importer::parse(&csv(&[&changed])).unwrap();
        assert_eq!(rows[0].disposition, expected);
        assert!(rows[0].purchase.is_none() && rows[0].repaired.is_some());
    }
}

#[test]
fn fresh_repairs_have_provenance_defaults_and_cutoff_without_staging() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .set_cutover(
            "2026-09-27",
            "Earlier records belong to historical spreadsheets",
        )
        .unwrap();
    let token = store
        .stage_review(Change::Default {
            merchant: "SYNTHETIC RAIL TOWN".into(),
            category: "Transport".into(),
            description: "Rail fare".into(),
        })
        .unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
    let bytes = csv(&[BAD, CITY]);
    let id = store.import("new.csv", &bytes).unwrap();
    assert_eq!(store.summary(id).unwrap().repairs.len(), 2);
    let rows = store.detail_rows().unwrap();
    assert_eq!(rows.iter().filter(|r| r.outstanding()).count(), 1);
    assert!(rows.iter().all(|r| !r.staged));
    assert_eq!(store.detail_row(1).unwrap().description, "Rail fare");
    assert_eq!(store.detail_row(2).unwrap().handling, "excluded");
    assert!(
        store
            .purchase_history(1)
            .unwrap()
            .iter()
            .any(|e| e.kind == "automatic CSV repair")
    );
    assert_eq!(store.import("repeat.csv", &bytes).unwrap(), id);
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT original_bytes FROM imports WHERE id=?1",
            [id],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        bytes
    );
}

#[test]
fn identical_legacy_reupload_recovers_once_and_preserves_current_staging() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .set_cutover("2026-09-27", "Historical boundary")
        .unwrap();
    let normal = BAD
        .replace("SYNTHETIC RAIL, TOWN", "OTHER SHOP")
        .replace("12345678900000000000001", "99999");
    store.import("normal.csv", &csv(&[&normal])).unwrap();
    store
        .edit_purchase(recon::queue::PurchaseEdit {
            id: 1,
            signature: store.detail_row(1).unwrap().signature(),
            account: "joint".into(),
            category: "Shopping".into(),
            description: "Keep my review".into(),
            use_category_default: false,
            use_description_default: false,
            allow_category: true,
            stage: true,
        })
        .unwrap();
    let before = store.detail_row(1).unwrap().signature();
    let bytes = csv(&[BAD, CITY]);
    let id = legacy(&dir, &bytes);
    assert_eq!(store.import("reupload.csv", &bytes).unwrap(), id);
    assert_eq!(store.unresolved_exception_count().unwrap(), 0);
    assert_eq!(store.detail_row(1).unwrap().signature(), before);
    assert!(store.detail_row(1).unwrap().staged);
    assert_eq!(store.queue_summary().unwrap().outstanding, 2);
    assert_eq!(store.summary(id).unwrap().repairs.len(), 2);
    assert!(
        store
            .summary(id)
            .unwrap()
            .exceptions
            .iter()
            .all(|e| e.resolution.contains("replaced by import"))
    );
    assert_eq!(
        store
            .review_page(&Filters {
                import_id: Some(id),
                view: "all".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        2
    );
    let revision = store.review_revision().unwrap();
    assert_eq!(store.repair_import(id).unwrap().repaired, 0);
    assert_eq!(store.review_revision().unwrap(), revision);
    drop(store);
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.import("again.csv", &bytes).unwrap(), id);
    assert_eq!(store.detail_rows().unwrap().len(), 3);
}

#[test]
fn recovery_failure_rolls_back_new_purchases_import_and_dispositions() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let id = legacy(&dir, &csv(&[BAD, CITY]));
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_repair BEFORE INSERT ON observation_repairs WHEN NEW.observation_id=2 BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(store.repair_import(id).is_err());
    assert_eq!(store.recent_imports().unwrap().len(), 1);
    assert!(store.detail_rows().unwrap().is_empty());
    assert_eq!(store.unresolved_exception_count().unwrap(), 2);
    assert!(store.summary(id).unwrap().repairs.is_empty());
}

#[test]
fn recovery_preserves_conflicts_and_existing_dispositions_and_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let corrected = BAD
        .replace("SYNTHETIC RAIL, TOWN", "SYNTHETIC RAIL  TOWN")
        .replace("4.01", "5.01");
    store.import("existing.csv", &csv(&[&corrected])).unwrap();
    store.decide(1, 0, "joint").unwrap();
    let id = legacy(&dir, &csv(&[BAD, CITY]));
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute("INSERT INTO exception_decisions(observation_id,reason) VALUES(3,'Already handled manually')",[]).unwrap();
    assert_eq!(store.repair_import(id).unwrap().repaired, 1);
    let row = store.detail_row(1).unwrap();
    assert!(row.conflict);
    assert_eq!((row.amount_pence, row.account.as_str()), (501, "joint"));
    assert_eq!(store.detail_rows().unwrap().len(), 1);
    assert_eq!(store.unresolved_exception_count().unwrap(), 1);
    let duplicate = legacy(&dir, &csv(&[CITY, CITY]));
    assert_eq!(store.repair_import(duplicate).unwrap().repaired, 0);
}
