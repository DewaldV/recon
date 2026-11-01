use std::sync::{Arc, Mutex};

use askama::Template;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Form, Multipart, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

use crate::{
    Error, Result,
    importer::MAX_UPLOAD,
    storage::{ImportSummary, Store},
};
mod workflow;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    store: Arc<Mutex<Store>>,
    worker: Arc<Semaphore>,
    bootstrap: String,
    session: String,
    csrf: String,
    execution: Arc<Semaphore>,
    execution_message: Mutex<String>,
}

impl AppState {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self(Arc::new(Inner {
            store: Arc::new(Mutex::new(store)),
            worker: Arc::new(Semaphore::new(1)),
            bootstrap: crate::config::random_secret()?,
            session: crate::config::random_secret()?,
            csrf: crate::config::random_secret()?,
            execution: Arc::new(Semaphore::new(1)),
            execution_message: Mutex::new(String::new()),
        })))
    }

    /// Deliberately explicit: only main prints this capability, never request logs.
    pub fn startup_url(&self) -> String {
        format!("http://127.0.0.1:7310/start#{}", self.0.bootstrap)
    }

    async fn work<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        // Bound queued work too, so a flood cannot spawn unlimited blocking tasks.
        let permit = self
            .0
            .worker
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Invalid("Recon is busy. Retry shortly.".into()))?;
        let store = self.0.store.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut store = store.lock().map_err(|_| Error::Worker)?;
            f(&mut store)
        })
        .await
        .map_err(|_| Error::Worker)?
    }
}

fn equal(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/start", get(start))
        .route("/session", post(session))
        .route("/health", get(|| async { "ok" }))
        .route(
            "/assets/htmx.min.js",
            get(|| async { asset("text/javascript", include_str!("../static/htmx.min.js")) }),
        )
        .route(
            "/assets/app.js",
            get(|| async { asset("text/javascript", include_str!("../static/app.js")) }),
        )
        .route(
            "/assets/app.css",
            get(|| async { asset("text/css", include_str!("../static/app.css")) }),
        )
        .route("/", get(home))
        .route("/imports", post(upload))
        .route("/imports/{id}", get(import_summary))
        .route("/purchases", get(legacy_purchases))
        .route("/review", get(review))
        .route("/merchants", get(workflow::merchants))
        .route("/rules", get(workflow::rules))
        .route("/run", get(workflow::prepare))
        .route("/runs", get(workflow::runs))
        .route("/runs/start", post(workflow::start))
        .route("/runs/{id}", get(workflow::run))
        .route("/runs/{id}/resume", post(workflow::resume))
        .route("/runs/{id}/notes/{operation}", post(workflow::note))
        .route("/review/stage", post(stage_change))
        .route("/review/catalogue", post(catalogue_upload))
        .route("/history/import", post(workflow::history_upload))
        .route("/review/previews/{token}", get(review_preview))
        .route("/review/previews/{token}/commit", post(commit_change))
        .route("/review/{id}/account", post(decide))
        .route(
            "/review/{id}/edit",
            get(workflow::edit).post(workflow::save),
        )
        .route("/review/{id}/history", get(purchase_history))
        .route("/recons", get(|| async { Redirect::permanent("/runs") }))
        .route("/recons/{id}", get(recon_summary))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD + 64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let mutation = !matches!(request.method().as_str(), "GET" | "HEAD");
    let public = matches!(
        request.uri().path(),
        "/start"
            | "/session"
            | "/health"
            | "/assets/htmx.min.js"
            | "/assets/app.js"
            | "/assets/app.css"
    );
    let authorized = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|cookie| {
            cookie.split(';').any(|pair| {
                pair.trim()
                    .strip_prefix("recon_session=")
                    .is_some_and(|value| equal(value, &state.0.session))
            })
        });
    let mut response = if host != Some("127.0.0.1:7310")
        || (mutation && origin != Some("http://127.0.0.1:7310"))
    {
        (StatusCode::FORBIDDEN, "Invalid local Host or Origin.").into_response()
    } else if !public && !authorized {
        (
            StatusCode::UNAUTHORIZED,
            "Open the startup link printed by Recon to sign in.",
        )
            .into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "same-origin"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

fn asset(kind: &'static str, text: &'static str) -> Response {
    ([(header::CONTENT_TYPE, kind)], text).into_response()
}

#[derive(Template)]
#[template(path = "start.html")]
struct Start;

async fn start() -> Result<Html<String>> {
    render(Start)
}

#[derive(Deserialize)]
struct SessionForm {
    token: String,
}

async fn session(State(state): State<AppState>, Form(form): Form<SessionForm>) -> Response {
    if !equal(&form.token, &state.0.bootstrap) {
        return (
            StatusCode::FORBIDDEN,
            "Invalid startup link. Use the link from this app process.",
        )
            .into_response();
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "recon_session={}; HttpOnly; SameSite=Strict; Path=/",
            state.0.session
        ))
        .unwrap(),
    );
    response
}

#[derive(Template)]
#[template(path = "home.html")]
struct Home {
    csrf: String,
    imports: Vec<(i64, String)>,
}

async fn home(State(state): State<AppState>) -> Result<Html<String>> {
    let imports = state.work(|s| s.recent_imports()).await?;
    render(Home {
        csrf: state.0.csrf.clone(),
        imports,
    })
}

async fn read_upload(state: &AppState, mut multipart: Multipart) -> Result<(String, Vec<u8>)> {
    let mut csrf = None;
    let mut file = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| Error::Invalid("Invalid or oversized upload (10 MiB file limit).".into()))?
    {
        match field.name() {
            Some("csrf") if csrf.is_none() => {
                csrf = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| Error::Invalid("Invalid upload form.".into()))?,
                );
            }
            Some("file") if file.is_none() => {
                let filename = field.file_name().unwrap_or("upload.csv").to_owned();
                if filename.len() > 255 {
                    return Err(Error::Invalid("Filename is too long.".into()));
                }
                let bytes = field.bytes().await.map_err(|_| {
                    Error::Invalid("Invalid or oversized upload (10 MiB file limit).".into())
                })?;
                if bytes.len() > MAX_UPLOAD {
                    return Err(Error::Invalid("CSV exceeds the 10 MiB limit.".into()));
                }
                file = Some((filename, bytes));
            }
            _ => return Err(Error::Invalid("Unexpected upload form field.".into())),
        }
    }
    if !csrf.is_some_and(|value| equal(&value, &state.0.csrf)) {
        return Err(Error::Invalid(
            "Invalid CSRF token. Reload the page.".into(),
        ));
    }
    let (filename, bytes) = file.ok_or_else(|| Error::Invalid("Choose a CSV to import.".into()))?;
    Ok((filename, bytes.to_vec()))
}

async fn upload(State(state): State<AppState>, multipart: Multipart) -> Result<Redirect> {
    let (filename, bytes) = read_upload(&state, multipart).await?;
    let id = state.work(move |s| s.import(&filename, &bytes)).await?;
    Ok(Redirect::to(&format!(
        "/review?view=attention&import_notice={id}"
    )))
}

#[derive(Template)]
#[template(path = "import.html")]
struct ImportPage {
    summary: ImportSummary,
    csrf: String,
}

async fn import_summary(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>> {
    render(ImportPage {
        csrf: state.0.csrf.clone(),
        summary: state.work(move |s| s.summary(id)).await?,
    })
}

async fn legacy_purchases(Query(filters): Query<crate::review::Filters>) -> Redirect {
    Redirect::permanent(&review_link(&filters, filters.page.unwrap_or(1)))
}

#[derive(Deserialize)]
struct DecisionForm {
    csrf: String,
    version: i64,
    account: String,
}

async fn decide(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(filters): Query<crate::review::Filters>,
    headers: HeaderMap,
    Form(form): Form<DecisionForm>,
) -> Result<Response> {
    if !equal(&form.csrf, &state.0.csrf) {
        return Err(Error::Invalid(
            "Invalid CSRF token. Reload the page.".into(),
        ));
    }
    let query = filters.clone();
    let (data, defaults) = state
        .work(move |s| {
            // Validate the return filters before committing any decision.
            s.review_page(&query)?;
            s.decide(id, form.version, &form.account)?;
            Ok((s.review_page(&query)?, s.merchant_default(&query.q)?))
        })
        .await?;
    let view = review_context(&state, filters, data, defaults);
    if is_htmx(&headers) {
        // Refresh counts, filter membership, pagination, and bulk versions together.
        Ok(render(ReviewWorkspace { view })?.into_response())
    } else {
        Ok(Redirect::to(&review_link(&view.filters, view.data.page)).into_response())
    }
}

fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some_and(|v| v == "true")
}

fn render(template: impl Template) -> Result<Html<String>> {
    template.render().map(Html).map_err(|_| Error::Worker)
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let code = match &self {
            Error::Invalid(_) => StatusCode::BAD_REQUEST,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Stale => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        // Error Display deliberately excludes SQL, paths, and financial input.
        let page = ErrorPage {
            message: self.to_string(),
        };
        match page.render() {
            Ok(html) => (code, Html(html)).into_response(),
            Err(_) => (code, "Recon could not complete this request.").into_response(),
        }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    message: String,
}

#[derive(Template)]
#[template(path = "review.html")]
struct Review {
    view: ReviewContext,
}
#[derive(Template)]
#[template(path = "review_workspace.html")]
struct ReviewWorkspace {
    view: ReviewContext,
}
struct ReviewContext {
    csrf: String,
    data: crate::review::ReviewPage,
    filters: crate::review::Filters,
    default_category: String,
    default_description: String,
    previous: String,
    next: String,
    query: String,
    return_to: String,
    active_run: Option<i64>,
    import_notice: Option<ImportSummary>,
}
async fn review(
    State(state): State<AppState>,
    Query(mut filters): Query<crate::review::Filters>,
    headers: HeaderMap,
) -> Result<Html<String>> {
    if filters.view.is_empty() {
        filters.view = "attention".into();
    }
    let query = filters.clone();
    let (data, defaults) = state
        .work(move |s| Ok((s.review_page(&query)?, s.merchant_default(&query.q)?)))
        .await?;
    let mut view = review_context(&state, filters, data, defaults);
    if let Some(id) = view.filters.import_notice {
        view.import_notice = Some(state.work(move |s| s.summary(id)).await?);
    }
    if is_htmx(&headers) {
        render(ReviewWorkspace { view })
    } else {
        render(Review { view })
    }
}

fn review_link(f: &crate::review::Filters, page: usize) -> String {
    fn encode(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    format!(
        "/review?q={}&account={}&missing={}&view={}&page={page}{}{}",
        encode(&f.q),
        encode(&f.account),
        encode(&f.missing),
        encode(&f.view),
        f.import_id
            .map(|id| format!("&import_id={id}"))
            .unwrap_or_default(),
        f.recon_id
            .map(|id| format!("&recon_id={id}"))
            .unwrap_or_default()
    )
}

fn review_context(
    state: &AppState,
    filters: crate::review::Filters,
    data: crate::review::ReviewPage,
    (default_category, default_description): (String, String),
) -> ReviewContext {
    let previous = review_link(&filters, data.page.saturating_sub(1).max(1));
    let next = review_link(&filters, data.page + 1);
    let query = review_link(&filters, data.page)
        .trim_start_matches("/review?")
        .to_owned();
    let return_to = review_link(&filters, data.page);
    let active_run = data.active_run;
    ReviewContext {
        csrf: state.0.csrf.clone(),
        data,
        filters,
        previous,
        next,
        default_category,
        default_description,
        query,
        return_to,
        active_run,
        import_notice: None,
    }
}
fn check_csrf(state: &AppState, form: &std::collections::HashMap<String, String>) -> Result<()> {
    if !form.get("csrf").is_some_and(|v| equal(v, &state.0.csrf)) {
        return Err(Error::Invalid(
            "Invalid CSRF token. Reload the page.".into(),
        ));
    }
    Ok(())
}
async fn stage_change(
    State(state): State<AppState>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Redirect> {
    use crate::review::Change;
    check_csrf(&state, &form)?;
    let value = |key: &str| form.get(key).cloned().unwrap_or_default();
    let mut rows = Vec::new();
    let mut signatures = Vec::new();
    for key in form.keys().filter_map(|k| k.strip_prefix("pick_")) {
        let (id, version) = key
            .split_once('_')
            .ok_or_else(|| Error::Invalid("Invalid selection.".into()))?;
        let id: i64 = id
            .parse()
            .map_err(|_| Error::Invalid("Invalid purchase ID.".into()))?;
        let version: i64 = version
            .parse()
            .map_err(|_| Error::Invalid("Invalid decision version.".into()))?;
        rows.push((id, version));
        signatures.push((id, value(&format!("pick_{id}_{version}"))));
    }
    rows.sort_unstable();
    let change = match value("action").as_str() {
        "stage" | "unstage" => Change::Staging {
            rows: signatures,
            staged: value("action") == "stage",
        },
        "account_rule" => Change::AccountRule {
            merchant: value("merchant"),
            card: value("card"),
            account: value("account"),
        },
        "exception" => Change::Exception {
            observation_id: value("observation_id")
                .parse()
                .map_err(|_| Error::Invalid("Invalid source observation.".into()))?,
            reason: value("reason"),
        },
        "default" => Change::Default {
            merchant: value("merchant"),
            category: value("category"),
            description: value("description"),
        },
        "details" => Change::Details {
            ids: rows.iter().map(|r| r.0).collect(),
            category: value("category"),
            description: value("description"),
        },
        "alias" => Change::Alias {
            merchant: value("merchant"),
            target: value("target"),
        },
        "accounts" => Change::Accounts {
            rows,
            account: value("account"),
        },
        "handling" => Change::Handling {
            ids: rows.iter().map(|r| r.0).collect(),
            state: value("handling"),
            reason: value("reason"),
            reopen: form.contains_key("reopen"),
        },
        _ => return Err(Error::Invalid("Choose a review action.".into())),
    };
    let return_to = workflow::return_path(&value("return_to"))?;
    let token = state
        .work(move |s| {
            let token = s.stage_review(change)?;
            s.db.execute(
                "INSERT INTO preview_context(token,return_to) VALUES(?1,?2)",
                rusqlite::params![token, return_to],
            )?;
            Ok(token)
        })
        .await?;
    Ok(Redirect::to(&format!("/review/previews/{token}")))
}
async fn catalogue_upload(State(state): State<AppState>, multipart: Multipart) -> Result<Redirect> {
    let (filename, bytes) = read_upload(&state, multipart).await?;
    let token = state
        .work(move |s| {
            let change = Store::parse_catalogue(filename, bytes)?;
            let token = s.stage_review(change)?;
            s.db.execute(
                "INSERT INTO preview_context(token,return_to) VALUES(?1,'/merchants')",
                [&token],
            )?;
            Ok(token)
        })
        .await?;
    Ok(Redirect::to(&format!("/review/previews/{token}")))
}
#[derive(Template)]
#[template(path = "preview.html")]
struct ReviewPreview {
    csrf: String,
    data: crate::review::Preview,
    title: String,
    merchant: String,
    category: String,
    description: String,
    account: String,
    target: String,
    catalogue: bool,
    operation: String,
    reason: String,
    reopen: bool,
}
async fn review_preview(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Html<String>> {
    use crate::review::Change;
    let data = state.work(move |s| s.review_preview(&token)).await?;
    let mut page = ReviewPreview {
        csrf: state.0.csrf.clone(),
        data,
        title: String::new(),
        merchant: String::new(),
        category: String::new(),
        description: String::new(),
        account: String::new(),
        target: String::new(),
        catalogue: false,
        operation: String::new(),
        reason: String::new(),
        reopen: false,
    };
    match &page.data.change {
        Change::Historical { filename, .. } => {
            page.title = format!("Import historical decisions: {filename}");
            page.operation="Confirm every reference match below. Existing explicit account, detail, handling, and run decisions are preserved. Legacy Paid = Yes records external funding only when handling is still unknown with no explicit decision. Paid = No never confirms outstanding status. Nothing is staged automatically.".into();
        }
        Change::Staging { staged, .. } => {
            page.title = if *staged {
                "Stage reviewed purchases"
            } else {
                "Return purchases to review"
            }
            .into();
            page.operation=if *staged {"Confirm these purchases still need funding and that the account, category, and note below are correct. They will join the next recon automatically."} else {"Return these purchases to review. They remain outstanding."}.into();
        }
        Change::AccountRule {
            merchant,
            card,
            account,
        } => {
            page.title = "Approve account rule".into();
            page.operation = format!(
                "Exact merchant: {merchant}; card: {} → {}. Explicit purchase choices are preserved. Changed effective decisions need staging again.",
                if card.is_empty() { "any" } else { card },
                if account.is_empty() {
                    "disabled"
                } else {
                    account
                }
            );
        }
        Change::Exception {
            observation_id,
            reason,
        } => {
            page.title = "Confirm source exception disposition".into();
            page.operation = format!(
                "Record the evidence for source observation #{observation_id}. For a conflict, retain the existing canonical purchase and reject this conflicting observation; otherwise exclude this source observation from funding. The original source is retained."
            );
            page.reason = reason.clone();
        }
        Change::Catalogue { filename, .. } => {
            page.title = format!("Import merchant catalogue: {filename}");
            page.catalogue = true;
        }
        Change::Default {
            merchant,
            category,
            description,
        } => {
            page.title = "Save merchant default".into();
            page.merchant = merchant.clone();
            page.category = category.clone();
            page.description = description.clone();
        }
        Change::Details {
            category,
            description,
            ..
        } => {
            page.title = "Set selected purchase overrides".into();
            page.category = category.clone();
            page.description = description.clone();
        }
        Change::Alias { merchant, target } => {
            page.title = "Approve exact merchant alias".into();
            page.merchant = merchant.clone();
            page.target = target.clone();
        }
        Change::Accounts { account, .. } => {
            page.title = "Assign selected purchases".into();
            page.account = if account.is_empty() {
                "Unset".into()
            } else {
                account.clone()
            };
        }
        Change::Handling {
            state: handling,
            reason,
            reopen,
            ..
        } => {
            page.title = "Set purchase handling state".into();
            page.operation = format!(
                "New handling: {handling}. External means already funded outside recon; do not deposit again. Unknown is blocked; outstanding is an explicit assertion, not payment approval."
            );
            page.reason = reason.clone();
            page.reopen = *reopen;
        }
        Change::CreateRecon { name } => {
            page.title = "Create recon workspace".into();
            page.operation = format!(
                "Create active recon: {name}. It starts empty; imports are never automatically selected."
            );
        }
        Change::Membership {
            recon_id,
            include,
            reason,
            ..
        } => {
            let id = *recon_id;
            let recon = state.work(move |s| s.recon_info(id)).await?;
            page.title = "Change explicit recon selection".into();
            page.operation = format!(
                "{} selected purchases {} recon #{}: {}. Handling history is unchanged.",
                if *include { "Add" } else { "Remove" },
                if *include { "to" } else { "from" },
                recon.id,
                recon.name
            );
            page.reason = reason.clone();
        }
        Change::ReconStatus {
            recon_id,
            status,
            reason,
        } => {
            let id = *recon_id;
            let recon = state.work(move |s| s.recon_info(id)).await?;
            page.title = "Change recon status".into();
            page.operation = format!(
                "Recon #{} {} → {status}. Archiving does not mark any purchase handled or paid; reopening does not override handling history.",
                recon.id, recon.name
            );
            page.reason = reason.clone();
        }
    }
    render(page)
}
async fn commit_change(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Redirect> {
    check_csrf(&state, &form)?;
    let mut selected = std::collections::BTreeSet::new();
    for key in form.keys().filter_map(|k| k.strip_prefix("record_")) {
        selected.insert(
            key.parse()
                .map_err(|_| Error::Invalid("Invalid catalogue selection.".into()))?,
        );
    }
    let allow = form.contains_key("allow_categories");
    let return_to = state
        .work(move |s| {
            let return_to = s.review_preview(&token)?.return_to;
            s.commit_review(&token, &selected, allow)?;
            Ok(return_to)
        })
        .await?;
    Ok(Redirect::to(&return_to))
}

#[derive(Template)]
#[template(path = "recon.html")]
struct ReconPage {
    summary: crate::recon::ReconSummary,
}

async fn recon_summary(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Html<String>> {
    render(ReconPage {
        summary: state.work(move |s| s.recon_summary(id)).await?,
    })
}

#[derive(Template)]
#[template(path = "history.html")]
struct HistoryPage {
    id: i64,
    events: Vec<crate::recon::HistoryEvent>,
}

async fn purchase_history(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>> {
    render(HistoryPage {
        id,
        events: state.work(move |s| s.purchase_history(id)).await?,
    })
}
