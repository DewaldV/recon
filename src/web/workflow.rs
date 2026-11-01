use super::*;
use crate::{
    execution::{self, Bank, Destination, Run},
    monzo::Monzo,
    queue::QueueSummary,
    review::{DetailRow, Filters},
    rules::{AccountRule, MerchantChoice},
};

pub(super) fn return_path(value: &str) -> Result<String> {
    if value.is_empty() {
        return Ok("/review".into());
    }
    if matches!(value, "/merchants" | "/rules" | "/review" | "/run") {
        return Ok(value.into());
    }
    if let Some(query) = value.strip_prefix("/review?") {
        let filters: Filters = serde_urlencoded::from_str(query)
            .map_err(|_| Error::Invalid("Invalid return filters.".into()))?;
        filters.validate()?;
        return Ok(review_link(&filters, filters.page.unwrap_or(1)));
    }
    if let Some(query) = value.strip_prefix("/rules?") {
        let query: RuleQuery = serde_urlencoded::from_str(query)
            .map_err(|_| Error::Invalid("Invalid rule selection.".into()))?;
        crate::rules::validate_rule(&query.merchant, &query.card, &query.account)?;
        return Ok(rule_link(&query.merchant, &query.card));
    }
    if let Some(id) = value
        .strip_prefix("/imports/")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|id| *id > 0)
    {
        return Ok(format!("/imports/{id}"));
    }
    Err(Error::Invalid("Invalid return page.".into()))
}

#[derive(Template)]
#[template(path = "merchants.html")]
struct Merchants {
    csrf: String,
    merchant: String,
    category: String,
    description: String,
    categories: Vec<String>,
    groups: Vec<(String, usize)>,
}
pub(super) async fn merchants(
    State(state): State<AppState>,
    Query(mut filters): Query<Filters>,
) -> Result<Html<String>> {
    filters.view = "all".into();
    let query = filters.clone();
    let (data, defaults) = state
        .work(move |s| Ok((s.review_page(&query)?, s.merchant_default(&query.q)?)))
        .await?;
    render(Merchants {
        csrf: state.0.csrf.clone(),
        merchant: filters.q,
        category: defaults.0,
        description: defaults.1,
        categories: data.categories,
        groups: data.groups,
    })
}
#[derive(Template)]
#[template(path = "rules.html")]
struct Rules {
    csrf: String,
    rules: Vec<AccountRule>,
    merchant: String,
    card: String,
    merchants: Vec<MerchantChoice>,
    cards: Vec<String>,
    seen_cards: std::collections::BTreeSet<String>,
    account: String,
    version: i64,
    return_to: String,
}
impl Rules {
    fn selected_card(&self, card: &str) -> bool {
        self.card == card
    }
    fn used_card(&self, card: &str) -> bool {
        self.seen_cards.contains(card)
    }
}
#[derive(Default, Deserialize)]
pub(super) struct RuleQuery {
    #[serde(default)]
    merchant: String,
    #[serde(default)]
    card: String,
    #[serde(default)]
    account: String,
}

fn rule_link(merchant: &str, card: &str) -> String {
    let query = serde_urlencoded::to_string([("merchant", merchant), ("card", card)])
        .expect("rule selection contains only strings");
    format!("/rules?{query}")
}

pub(super) async fn rules(
    State(state): State<AppState>,
    Query(q): Query<RuleQuery>,
) -> Result<Html<String>> {
    let (rules, choices, merchant, card, account, version, seen_cards) = state
        .work(move |s| {
            if !matches!(q.account.as_str(), "" | "personal" | "joint") {
                return Err(Error::Invalid("Invalid suggested account.".into()));
            }
            let merchant = if q.merchant.is_empty() {
                String::new()
            } else {
                s.canonical_rule_merchant(&q.merchant)?
            };
            let choices = s.rule_choices()?;
            let card = q.card.trim().to_owned();
            if !card.is_empty() && !choices.cards.contains(&card) {
                return Err(Error::Invalid(
                    "Choose a card from your imported transactions or existing rules.".into(),
                ));
            }
            let key = crate::review::normalize(&merchant);
            let rules = s.account_rules()?;
            let current = rules.iter().find(|r| r.key == key && r.card == card);
            let account = current.map(|r| r.account.clone()).unwrap_or_else(|| {
                if q.account.is_empty() {
                    "personal".into()
                } else {
                    q.account
                }
            });
            let version = current.map_or(0, |r| r.version);
            let seen_cards = choices
                .merchants
                .iter()
                .find(|m| m.key == key)
                .map(|m| m.cards.clone())
                .unwrap_or_default();
            Ok((rules, choices, merchant, card, account, version, seen_cards))
        })
        .await?;
    let return_to = rule_link(&merchant, &card);
    render(Rules {
        csrf: state.0.csrf.clone(),
        rules,
        merchant,
        card,
        account,
        version,
        seen_cards,
        return_to,
        merchants: choices.merchants,
        cards: choices.cards,
    })
}

#[derive(Template)]
#[template(path = "run_prepare.html")]
struct Prepare {
    csrf: String,
    summary: QueueSummary,
    rows: Vec<DetailRow>,
    revision: i64,
    active: Option<i64>,
    personal_account: String,
    personal_pot: String,
    joint_account: String,
    joint_pot: String,
}
pub(super) async fn prepare(State(state): State<AppState>) -> Result<Html<String>> {
    let (summary, rows, revision, active, previous) = state
        .work(|s| {
            let rows = s.detail_rows()?;
            let summary = QueueSummary::from_rows(&rows, s.unresolved_exception_count()?)?;
            Ok((
                summary,
                rows.into_iter().filter(|r| r.outstanding()).collect(),
                s.review_revision()?,
                s.active_run()?,
                s.run_ids()?.first().map(|id| s.run(*id)).transpose()?,
            ))
        })
        .await?;
    let target = |allocation: &str| {
        previous
            .as_ref()
            .and_then(|r| {
                r.operations
                    .iter()
                    .find(|o| o.destination.allocation == allocation)
            })
            .map(|o| {
                (
                    o.destination.account_id.clone(),
                    o.destination.pot_id.clone(),
                )
            })
            .unwrap_or_default()
    };
    let (personal_account, personal_pot) = target("personal");
    let (joint_account, joint_pot) = target("joint");
    render(Prepare {
        csrf: state.0.csrf.clone(),
        summary,
        rows,
        revision,
        active,
        personal_account,
        personal_pot,
        joint_account,
        joint_pot,
    })
}

#[derive(Deserialize)]
pub(super) struct StartForm {
    csrf: String,
    revision: i64,
    token: String,
    #[serde(default)]
    personal_account: String,
    #[serde(default)]
    personal_pot: String,
    #[serde(default)]
    joint_account: String,
    #[serde(default)]
    joint_pot: String,
}
pub(super) async fn start(
    State(state): State<AppState>,
    Form(form): Form<StartForm>,
) -> Result<Redirect> {
    state.check_csrf(&form.csrf)?;
    let permit = state.execution_permit()?;
    let revision = form.revision;
    let summary = state
        .work(move |s| {
            s.verify_execution_guard()?;
            if s.review_revision()? != revision {
                return Err(Error::Stale);
            }
            if s.active_run()?.is_some() {
                return Err(Error::Invalid("Resume the current recon.".into()));
            }
            let summary = s.queue_summary()?;
            if !summary.runnable() {
                return Err(Error::Invalid(
                    "Complete review and source exceptions first.".into(),
                ));
            }
            Ok(summary)
        })
        .await?;
    let mut targets = Vec::new();
    if summary.personal_count > 0 {
        targets.push((
            Destination {
                allocation: "personal".into(),
                account_id: form.personal_account,
                pot_id: form.personal_pot,
            },
            summary.personal_pence,
        ));
    }
    if summary.joint_count > 0 {
        targets.push((
            Destination {
                allocation: "joint".into(),
                account_id: form.joint_account,
                pot_id: form.joint_pot,
            },
            summary.joint_pence,
        ));
    }
    for (target, _) in &targets {
        target.validate()?;
    }
    let bank = Monzo::new(form.token)?;
    bank.preflight(&targets)
        .await
        .map_err(|e| Error::Invalid(e.message().into()))?;
    let id = state
        .work(move |s| {
            s.prepare_run(
                revision,
                &targets.into_iter().map(|t| t.0).collect::<Vec<_>>(),
            )
        })
        .await?;
    launch(state, bank, id, permit);
    Ok(Redirect::to(&format!("/runs/{id}")))
}
fn launch(state: AppState, bank: Monzo, id: i64, permit: tokio::sync::OwnedSemaphorePermit) {
    if let Ok(mut message) = state.0.execution_message.lock() {
        *message = String::new();
    }
    tokio::spawn(async move {
        let _permit = permit;
        if let Err(e) = execution::execute(state.0.store.clone(), &bank, id).await
            && let Ok(mut message) = state.0.execution_message.lock()
        {
            *message = e.to_string();
        }
    });
}
#[derive(Deserialize)]
pub(super) struct TokenForm {
    csrf: String,
    token: String,
    #[serde(default)]
    transaction: String,
    #[serde(default)]
    confirmed: Option<String>,
}
pub(super) async fn resume(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<TokenForm>,
) -> Result<Redirect> {
    state.check_csrf(&form.csrf)?;
    let permit = state.execution_permit()?;
    state
        .work(move |s| {
            if s.active_run()? != Some(id) {
                return Err(Error::Invalid("This recon is not active.".into()));
            }
            Ok(())
        })
        .await?;
    launch(state, Monzo::new(form.token)?, id, permit);
    Ok(Redirect::to(&format!("/runs/{id}")))
}
pub(super) async fn note(
    State(state): State<AppState>,
    Path((id, operation)): Path<(i64, i64)>,
    Form(form): Form<TokenForm>,
) -> Result<Redirect> {
    state.check_csrf(&form.csrf)?;
    if form.confirmed.is_none() {
        return Err(Error::Invalid(
            "Confirm the exact Monzo transaction belongs to this deposit.".into(),
        ));
    }
    let _permit = state.execution_permit()?;
    execution::apply_note(
        state.0.store.clone(),
        &Monzo::new(form.token)?,
        id,
        operation,
        form.transaction,
    )
    .await?;
    Ok(Redirect::to(&format!("/runs/{id}")))
}

#[derive(Template)]
#[template(path = "runs.html")]
struct Runs {
    runs: Vec<Run>,
    legacy: Vec<crate::recon::ReconInfo>,
}
pub(super) async fn runs(State(state): State<AppState>) -> Result<Html<String>> {
    let (runs, legacy) = state
        .work(|s| {
            Ok((
                s.run_ids()?
                    .into_iter()
                    .map(|id| s.run(id))
                    .collect::<Result<_>>()?,
                s.recons()?,
            ))
        })
        .await?;
    render(Runs { runs, legacy })
}
#[derive(Template)]
#[template(path = "run.html")]
struct RunPage {
    csrf: String,
    run: Run,
    running: bool,
    message: String,
}
pub(super) async fn run(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>> {
    let running = state.0.execution.available_permits() == 0;
    let message = state
        .0
        .execution_message
        .lock()
        .map_err(|_| Error::Worker)?
        .clone();
    render(RunPage {
        csrf: state.0.csrf.clone(),
        run: state.work(move |s| s.run(id)).await?,
        running,
        message,
    })
}

#[derive(Template)]
#[template(path = "purchase_edit.html")]
struct EditPage {
    csrf: String,
    row: DetailRow,
    return_to: String,
    categories: Vec<String>,
    reference: String,
    posting: String,
    imports: Vec<(i64, String)>,
}
pub(super) async fn edit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(filters): Query<Filters>,
) -> Result<Html<String>> {
    filters.validate()?;
    let (row, categories, reference, posting, imports) = state
        .work(move |s| {
            let row = s.detail_row(id)?;
            let (reference, posting) = s.db.query_row(
                "SELECT reference,posting_date FROM purchases WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let imports =
                s.db.prepare(
                    "SELECT DISTINCT i.id,i.original_filename FROM imports i
                 JOIN source_observations o ON o.import_id=i.id
                 WHERE o.purchase_id=?1 ORDER BY i.id",
                )?
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok((row, s.categories()?, reference, posting, imports))
        })
        .await?;
    render(EditPage {
        csrf: state.0.csrf.clone(),
        row,
        return_to: review_link(&filters, filters.page.unwrap_or(1)),
        categories,
        reference,
        posting,
        imports,
    })
}
#[derive(Deserialize)]
pub(super) struct EditForm {
    csrf: String,
    signature: String,
    return_to: String,
    account: String,
    category: String,
    description: String,
    action: String,
    use_category_default: Option<String>,
    use_description_default: Option<String>,
    allow_category: Option<String>,
}
pub(super) async fn save(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<EditForm>,
) -> Result<Redirect> {
    state.check_csrf(&form.csrf)?;
    let return_to = return_path(&form.return_to)?;
    if !matches!(form.action.as_str(), "save" | "stage") {
        return Err(Error::Invalid("Choose save or stage.".into()));
    }
    state
        .work(move |s| {
            s.edit_purchase(crate::queue::PurchaseEdit {
                id,
                signature: form.signature,
                account: form.account,
                category: form.category,
                description: form.description,
                use_category_default: form.use_category_default.is_some(),
                use_description_default: form.use_description_default.is_some(),
                allow_category: form.allow_category.is_some(),
                stage: form.action == "stage",
            })
        })
        .await?;
    Ok(Redirect::to(&return_to))
}

pub(super) async fn history_upload(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Redirect> {
    let (filename, bytes) = read_upload(&state, multipart).await?;
    let token = state
        .work(move |s| {
            let change = s.parse_history(filename, bytes)?;
            s.stage_review(change)
        })
        .await?;
    Ok(Redirect::to(&format!("/review/previews/{token}")))
}
