use recon::{review::Change, storage::Store};
use std::collections::BTreeSet;

fn apply(store: &mut Store, change: Change) {
    let token = store.stage_review(change).unwrap();
    store.commit_review(&token, &BTreeSet::new(), true).unwrap();
}
fn rule(card: &str, account: &str) -> Change {
    Change::AccountRule {
        merchant: "SYNTHETIC SHOP".into(),
        card: card.into(),
        account: account.into(),
    }
}
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    store
        .import("synthetic", include_bytes!("fixtures/virgin-first.csv"))
        .unwrap();
    apply(
        &mut store,
        Change::Default {
            merchant: "SYNTHETIC SHOP".into(),
            category: "Shopping".into(),
            description: "Groceries".into(),
        },
    );
    (dir, store)
}
fn stage_all(store: &mut Store) {
    let rows = store
        .detail_rows()
        .unwrap()
        .into_iter()
        .map(|r| (r.id, r.signature()))
        .collect();
    apply(store, Change::Staging { rows, staged: true });
}

#[test]
fn choices_include_catalogue_only_merchants_aliases_and_existing_rule_cards() {
    let (_dir, mut store) = setup();
    apply(
        &mut store,
        Change::Default {
            merchant: "Catalogue Only".into(),
            category: "Shopping".into(),
            description: "Future purchase".into(),
        },
    );
    apply(
        &mut store,
        Change::Alias {
            merchant: "Shop Alias".into(),
            target: "SYNTHETIC SHOP".into(),
        },
    );
    apply(&mut store, rule("9876", "joint"));
    let choices = store.rule_choices().unwrap();
    assert_eq!(choices.cards, vec!["0000", "9876"]);
    let shop = choices
        .merchants
        .iter()
        .find(|m| m.key == "synthetic shop")
        .unwrap();
    assert_eq!(shop.aliases, "Shop Alias");
    assert_eq!(shop.cards, BTreeSet::from(["0000".into()]));
    assert!(!choices.merchants.iter().any(|m| m.key == "shop alias"));
    assert!(
        choices
            .merchants
            .iter()
            .any(|m| m.name == "Catalogue Only" && m.cards.is_empty())
    );
    assert_eq!(
        store.canonical_rule_merchant(" shop alias ").unwrap(),
        "SYNTHETIC SHOP"
    );
    assert!(store.canonical_rule_merchant("Unknown").is_err());
}

#[test]
fn impact_preserves_explicit_choices_and_predicts_actual_restaging() {
    let (_dir, mut store) = setup();
    apply(&mut store, rule("", "personal"));
    store.decide(1, 0, "joint").unwrap();
    stage_all(&mut store);
    let token = store.stage_review(rule("", "joint")).unwrap();
    let impact = store.review_preview(&token).unwrap().rule_impact.unwrap();
    assert_eq!(
        (impact.changed, impact.explicit_choices, impact.restage),
        (1, 1, 1)
    );
    store
        .commit_review(&token, &BTreeSet::new(), false)
        .unwrap();
    for expected in impact.purchases {
        let actual = store.detail_row(expected.id).unwrap();
        assert_eq!(actual.account, expected.after);
        assert_eq!(!actual.staged, expected.restage);
    }
}

#[test]
fn card_scope_and_disable_preview_respect_rule_precedence_and_aliases() {
    let (dir, mut store) = setup();
    let db = rusqlite::Connection::open(dir.path().join("recon.sqlite3")).unwrap();
    db.execute("INSERT INTO purchases(source_account_id,reference,transaction_date,posting_date,amount_pence,currency,merchant,card_used,original_currency) VALUES(1,'extra','2026-10-01','2026-10-02',100,'GBP','Shop Alias','1111','GBP')",[]).unwrap();
    apply(
        &mut store,
        Change::Alias {
            merchant: "Shop Alias".into(),
            target: "SYNTHETIC SHOP".into(),
        },
    );
    apply(&mut store, rule("", "personal"));
    apply(&mut store, rule("0000", "joint"));
    stage_all(&mut store);
    let token = store.stage_review(rule("", "joint")).unwrap();
    let impact = store.review_preview(&token).unwrap().rule_impact.unwrap();
    assert_eq!(
        (impact.purchases.len(), impact.changed, impact.restage),
        (3, 1, 1)
    );
    assert!(impact.purchases.iter().find(|r| r.id == 3).unwrap().restage);
    // Disabling the specific rule falls back to the existing merchant-wide Personal rule.
    let token = store.stage_review(rule("0000", "")).unwrap();
    let impact = store.review_preview(&token).unwrap().rule_impact.unwrap();
    assert_eq!(
        (impact.purchases.len(), impact.changed, impact.restage),
        (2, 2, 2)
    );
    assert!(impact.purchases.iter().all(|r| r.after == "personal"));
    store
        .commit_review(&token, &BTreeSet::new(), false)
        .unwrap();
    assert!(store.detail_row(3).unwrap().staged);
    for expected in impact.purchases {
        let actual = store.detail_row(expected.id).unwrap();
        assert_eq!(actual.account, expected.after);
        assert!(!actual.staged);
    }
}

#[test]
fn same_account_rule_revision_still_predicts_restaging_and_runs_are_preserved() {
    let (_dir, mut store) = setup();
    apply(&mut store, rule("", "personal"));
    stage_all(&mut store);
    let token = store.stage_review(rule("", "personal")).unwrap();
    let impact = store.review_preview(&token).unwrap().rule_impact.unwrap();
    assert_eq!((impact.changed, impact.restage), (2, 2));
    apply(
        &mut store,
        Change::Exception {
            observation_id: 3,
            reason: "Synthetic non-funding evidence".into(),
        },
    );
    store
        .prepare_run(
            store.review_revision().unwrap(),
            &[recon::execution::Destination {
                allocation: "personal".into(),
                account_id: "acc_test".into(),
                pot_id: "pot_test".into(),
            }],
        )
        .unwrap();
    let token = store.stage_review(rule("", "joint")).unwrap();
    let impact = store.review_preview(&token).unwrap().rule_impact.unwrap();
    assert_eq!(
        (impact.changed, impact.restage, impact.fixed_runs),
        (0, 0, 2)
    );
    assert!(impact.purchases.iter().all(|r| r.after == "personal"));
}
