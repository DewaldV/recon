use recon::{
    Error,
    review::{Change, Filters, normalize},
    storage::Store,
};
use rusqlite::Connection;
use std::collections::BTreeSet;
const FIRST: &[u8] = include_bytes!("fixtures/virgin-first.csv");
fn apply(store: &mut Store, change: Change) {
    let token = store.stage_review(change).unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
}
fn default(description: &str) -> Change {
    Change::Default {
        merchant: " synthetic   SHOP ".into(),
        category: "Shopping".into(),
        description: description.into(),
    }
}
fn catalogue(bytes: &[u8]) -> Change {
    Store::parse_catalogue("synthetic-merchants.csv".into(), bytes.to_vec()).unwrap()
}

#[test]
fn defaults_overrides_clear_overlap_restart_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("first", FIRST).unwrap();
    apply(&mut store, default("Original 🛍️"));
    assert!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .all(|r| r.category == "Shopping"
                && r.description == "Original 🛍️"
                && r.account.is_empty())
    );
    apply(
        &mut store,
        Change::Details {
            ids: vec![1],
            category: "Personal category".into(),
            description: "Explicit".into(),
        },
    );
    apply(&mut store, default("Updated"));
    store
        .import("overlap", include_bytes!("fixtures/virgin-overlap.csv"))
        .unwrap();
    let rows = store.detail_rows().unwrap();
    let row = rows.iter().find(|r| r.id == 1).unwrap();
    assert_eq!(row.description, "Explicit");
    assert_eq!(row.category_source, "purchase override v1");
    assert_eq!(
        rows.iter().find(|r| r.id == 2).unwrap().description,
        "Updated"
    );
    apply(
        &mut store,
        Change::Details {
            ids: vec![1],
            category: " ".into(),
            description: "".into(),
        },
    );
    assert_eq!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 1)
            .unwrap()
            .description,
        "Updated"
    );
    let backup = store.backup().unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(
        store.merchant_default("SYNTHETIC SHOP").unwrap().1,
        "Updated"
    );
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(backup, restored.path().join("recon.sqlite3")).unwrap();
    let store = Store::open(restored.path()).unwrap();
    assert_eq!(store.detail_rows().unwrap().len(), 3);
    assert_eq!(
        store.merchant_default("SYNTHETIC SHOP").unwrap().1,
        "Updated"
    );
}
#[test]
fn catalogue_conflicts_preview_provenance_and_idempotence() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("first", FIRST).unwrap();
    let bytes=b"Merchant,Category,Description\nSYNTHETIC SHOP,Shopping,First\n synthetic  shop ,Shopping,First\nSYNTHETIC SHOP,Other,Second\nUnknown,,\n";
    let token = store.stage_review(catalogue(bytes)).unwrap();
    let preview = store.review_preview(&token).unwrap();
    assert_eq!(preview.affected.len(), 2);
    assert!(!preview.catalogue[0].2);
    assert!(preview.catalogue[0].1.contains("Conflict"));
    assert!(
        store
            .commit_review(&token, &BTreeSet::from([2, 4]), true)
            .is_err()
    );
    assert!(store.categories().unwrap().is_empty());
    store
        .commit_review(&token, &BTreeSet::from([2, 5]), true)
        .unwrap();
    apply(&mut store, default("Later edit"));
    let repeat = store.stage_review(catalogue(bytes)).unwrap();
    assert!(store.review_preview(&repeat).unwrap().duplicate);
    store
        .commit_review(&repeat, &BTreeSet::from([4]), true)
        .unwrap();
    assert_eq!(
        store.merchant_default("SYNTHETIC SHOP").unwrap().1,
        "Later edit"
    );
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    let (archived,record):(Vec<u8>,i64)=db.query_row("SELECT s.bytes,d.source_record FROM merchant_sources s JOIN merchant_definitions d ON d.source_id=s.id WHERE d.merchant_key='synthetic shop'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(archived, bytes);
    assert_eq!(record, 2);
    assert!(
        db.execute("UPDATE merchant_definitions SET description='bad'", [])
            .is_err()
    );
}
#[test]
fn identical_catalogue_rows_coalesce_and_categories_need_approval() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let token = store
        .stage_review(catalogue(
            b"Merchant,Category,Description\nShop,Shopping,Test\n shop ,Shopping,Test\n",
        ))
        .unwrap();
    let preview = store.review_preview(&token).unwrap();
    assert!(preview.catalogue[0].2);
    assert!(!preview.catalogue[1].2);
    assert!(
        store
            .commit_review(&token, &BTreeSet::from([2]), false)
            .is_err()
    );
    assert!(store.categories().unwrap().is_empty());
    store
        .commit_review(&token, &BTreeSet::from([2]), true)
        .unwrap();
    apply(
        &mut store,
        Change::Default {
            merchant: "Other".into(),
            category: " shopping ".into(),
            description: "Other".into(),
        },
    );
    assert_eq!(store.categories().unwrap(), vec!["Shopping"]);
}
#[test]
fn stale_previews_and_atomic_bulk_decisions() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("first", FIRST).unwrap();
    let token = store
        .stage_review(Change::Accounts {
            rows: vec![(1, 0), (2, 0)],
            account: "joint".into(),
        })
        .unwrap();
    store.decide(2, 0, "personal").unwrap();
    assert!(matches!(
        store.commit_review(&token, &BTreeSet::new(), false),
        Err(Error::Stale)
    ));
    assert!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 1)
            .unwrap()
            .account
            .is_empty()
    );
    assert!(
        store
            .stage_review(Change::Accounts {
                rows: vec![(1, 0), (2, 0)],
                account: "joint".into()
            })
            .is_err()
    );
    apply(
        &mut store,
        Change::Accounts {
            rows: vec![(1, 0), (2, 1)],
            account: "joint".into(),
        },
    );
    assert!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .all(|r| r.account == "joint")
    );
    let token = store.stage_review(default("Preview")).unwrap();
    store
        .import("overlap", include_bytes!("fixtures/virgin-overlap.csv"))
        .unwrap();
    assert!(matches!(store.review_preview(&token), Err(Error::Stale)));
}
#[test]
fn filters_counts_aliases_and_conflicts() {
    assert_eq!(normalize(" A  B\t123! "), "a b 123!");
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("first", FIRST).unwrap();
    let page = store
        .review_page(&Filters {
            view: "attention".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        (
            page.total,
            page.missing_account,
            page.missing_category,
            page.missing_description
        ),
        (2, 2, 2, 2)
    );
    apply(&mut store, default("Details"));
    apply(
        &mut store,
        Change::Accounts {
            rows: vec![(1, 0), (2, 0)],
            account: "personal".into(),
        },
    );
    assert_eq!(
        store
            .review_page(&Filters {
                view: "complete".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        2
    );
    apply(
        &mut store,
        Change::Default {
            merchant: "TARGET".into(),
            category: "Other".into(),
            description: "Target description".into(),
        },
    );
    apply(
        &mut store,
        Change::Alias {
            merchant: "SYNTHETIC CAFE, TEST".into(),
            target: "TARGET".into(),
        },
    );
    store
        .import("overlap", include_bytes!("fixtures/virgin-overlap.csv"))
        .unwrap();
    assert_eq!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 3)
            .unwrap()
            .description,
        "Target description"
    );
    assert!(
        store
            .stage_review(Change::Alias {
                merchant: "TARGET".into(),
                target: "SYNTHETIC CAFE, TEST".into()
            })
            .is_err()
    );
    let changed = String::from_utf8(FIRST.to_vec())
        .unwrap()
        .replace("12.34", "99.00");
    store.import("conflict", changed.as_bytes()).unwrap();
    let page = store
        .review_page(&Filters {
            missing: "conflict".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(
        store
            .review_page(&Filters {
                view: "complete".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        0
    );
    assert!(
        store
            .stage_review(Change::Accounts {
                rows: vec![(1, 1), (3, 0)],
                account: "joint".into()
            })
            .is_err()
    );
    assert_eq!(
        store
            .detail_rows()
            .unwrap()
            .iter()
            .find(|r| r.id == 3)
            .unwrap()
            .account,
        ""
    );
}
#[test]
fn malformed_catalogues_and_migration_keep_existing_decisions() {
    for bytes in [
        b"wrong,headers\na,b\n".as_slice(),
        b"Merchant,Category,Description\n,Test,Test\n",
        b"Merchant,Category,Description\nTest,Test\n",
        b"\xff",
    ] {
        assert!(Store::parse_catalogue("bad".into(), bytes.to_vec()).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recon.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../migrations/0001_initial.sql"))
        .unwrap();
    db.execute_batch(include_str!("../migrations/0002_original_currency.sql"))
        .unwrap();
    db.execute_batch("INSERT INTO source_accounts VALUES(1,'virgin','virgin-primary'); INSERT INTO purchases(id,source_account_id,reference,transaction_date,posting_date,amount_pence,currency,merchant,card_used,original_currency) VALUES(1,1,'r','2026-01-01','2026-01-02',100,'GBP','Shop','Test','GBP'); INSERT INTO review_decisions(purchase_id,version,account) VALUES(1,1,'joint'); PRAGMA user_version=2;").unwrap();
    drop(db);
    let store = Store::open(dir.path()).unwrap();
    let row = &store.detail_rows().unwrap()[0];
    assert_eq!(row.account, "joint");
    assert_eq!(row.version, 1);
    assert_eq!(row.category, "");
    assert_eq!(
        std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn storage_failure_rolls_back_bulk_and_catalogue_commits() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store.import("first", FIRST).unwrap();
    let db = Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    let token = store
        .stage_review(Change::Accounts {
            rows: vec![(1, 0), (2, 0)],
            account: "joint".into(),
        })
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_bulk BEFORE INSERT ON review_decisions WHEN NEW.purchase_id=2 BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(
        store
            .commit_review(&token, &BTreeSet::new(), false)
            .is_err()
    );
    assert!(store.detail_rows().unwrap().iter().all(|r| r.version == 0));
    db.execute_batch("DROP TRIGGER fail_bulk;").unwrap();
    store
        .commit_review(&token, &BTreeSet::new(), false)
        .unwrap();
    let token=store.stage_review(catalogue(b"Merchant,Category,Description\nFirst,New category,First\nSecond,New category,Second\n")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_catalogue BEFORE INSERT ON merchant_definitions WHEN NEW.merchant_key='second' BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(
        store
            .commit_review(&token, &BTreeSet::from([2, 3]), true)
            .is_err()
    );
    assert!(store.categories().unwrap().is_empty());
    let count: i64 = db
        .query_row("SELECT count(*) FROM merchant_sources", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    db.execute_batch("DROP TRIGGER fail_catalogue;").unwrap();
    store
        .commit_review(&token, &BTreeSet::from([2, 3]), true)
        .unwrap();
}

#[test]
fn review_pagination_membership_and_selection_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let first = String::from_utf8(FIRST.to_vec()).unwrap();
    let mut csv = format!("{}\n", first.lines().next().unwrap());
    for id in 1..=101 {
        csv.push_str(
            &first
                .lines()
                .nth(1)
                .unwrap()
                .replace("00000000000000000000001", &format!("{id:023}")),
        );
        csv.push('\n');
    }
    let import = store.import("many", csv.as_bytes()).unwrap();
    let page = store
        .review_page(&Filters {
            page: Some(2),
            import_id: Some(import),
            q: "synthetic".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.total, 101);
    assert_eq!(page.rows.len(), 50);
    assert_eq!(page.pages, 3);
    assert!(
        store
            .stage_review(Change::Accounts {
                rows: (1..=101).map(|id| (id, 0)).collect(),
                account: "joint".into()
            })
            .is_err()
    );
    let selected = page.rows.iter().map(|r| (r.id, r.version)).collect();
    apply(
        &mut store,
        Change::Accounts {
            rows: selected,
            account: "joint".into(),
        },
    );
    assert_eq!(
        store
            .review_page(&Filters {
                account: "joint".into(),
                ..Default::default()
            })
            .unwrap()
            .total,
        50
    );
    assert_eq!(
        store
            .review_page(&Filters {
                import_id: Some(999),
                ..Default::default()
            })
            .unwrap()
            .total,
        0
    );
}
