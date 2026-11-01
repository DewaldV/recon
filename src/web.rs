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
    storage::{ImportSummary, PurchasePage, PurchaseRow, Store},
};

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    store: Arc<Mutex<Store>>,
    worker: Arc<Semaphore>,
    bootstrap: String,
    session: String,
    csrf: String,
}

impl AppState {
    pub fn new(store: Store) -> Result<Self> {
        Ok(Self(Arc::new(Inner {
            store: Arc::new(Mutex::new(store)),
            worker: Arc::new(Semaphore::new(1)),
            bootstrap: crate::config::random_secret()?,
            session: crate::config::random_secret()?,
            csrf: crate::config::random_secret()?,
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
        .route("/purchases", get(purchases))
        .route("/review", get(review))
        .route("/review/stage", post(stage_change))
        .route("/review/catalogue", post(catalogue_upload))
        .route("/review/previews/{token}", get(review_preview))
        .route("/review/previews/{token}/commit", post(commit_change))
        .route("/purchases/{id}/account", post(decide))
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

async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> Result<Redirect> {
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
                let filename = field.file_name().unwrap_or("transactions.csv").to_owned();
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
    let (filename, bytes) =
        file.ok_or_else(|| Error::Invalid("Choose a Virgin CSV to import.".into()))?;
    let id = state.work(move |s| s.import(&filename, &bytes)).await?;
    Ok(Redirect::to(&format!("/imports/{id}")))
}

#[derive(Template)]
#[template(path = "import.html")]
struct ImportPage {
    summary: ImportSummary,
}

async fn import_summary(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>> {
    render(ImportPage {
        summary: state.work(move |s| s.summary(id)).await?,
    })
}

#[derive(Default, Deserialize)]
struct PageQuery {
    page: Option<i64>,
}

#[derive(Template)]
#[template(path = "purchases.html")]
struct Purchases {
    csrf: String,
    data: PurchasePage,
}

#[derive(Template)]
#[template(path = "table.html")]
struct Table {
    csrf: String,
    data: PurchasePage,
}

async fn purchases(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
    headers: HeaderMap,
) -> Result<Html<String>> {
    let data = state
        .work(move |s| s.purchases(query.page.unwrap_or(1)))
        .await?;
    if is_htmx(&headers) {
        render(Table {
            csrf: state.0.csrf.clone(),
            data,
        })
    } else {
        render(Purchases {
            csrf: state.0.csrf.clone(),
            data,
        })
    }
}

#[derive(Deserialize)]
struct DecisionForm {
    csrf: String,
    version: i64,
    account: String,
    page: Option<i64>,
}

#[derive(Template)]
#[template(path = "row.html")]
struct Row {
    csrf: String,
    row: PurchaseRow,
    page: i64,
}

async fn decide(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<DecisionForm>,
) -> Result<Response> {
    if !equal(&form.csrf, &state.0.csrf) {
        return Err(Error::Invalid(
            "Invalid CSRF token. Reload the page.".into(),
        ));
    }
    let page = form.page.unwrap_or(1).max(1);
    let row = state
        .work(move |s| s.decide(id, form.version, &form.account))
        .await?;
    if is_htmx(&headers) {
        Ok(render(Row {
            csrf: state.0.csrf.clone(),
            row,
            page,
        })?
        .into_response())
    } else {
        Ok(Redirect::to(&format!("/purchases?page={page}")).into_response())
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
        (code, self.to_string()).into_response()
    }
}

#[derive(Template)]
#[template(path = "review.html")]
struct Review {
    csrf: String,
    data: crate::review::ReviewPage,
    filters: crate::review::Filters,
    default_category: String,
    default_description: String,
    previous: String,
    next: String,
}
async fn review(
    State(state): State<AppState>,
    Query(filters): Query<crate::review::Filters>,
) -> Result<Html<String>> {
    let query = filters.clone();
    let data = state.work(move |s| s.review_page(&query)).await?;
    fn link(f: &crate::review::Filters, page: usize) -> String {
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
            "/review?q={}&account={}&missing={}&view={}&page={page}{}",
            encode(&f.q),
            encode(&f.account),
            encode(&f.missing),
            encode(&f.view),
            f.import_id
                .map(|i| format!("&import_id={i}"))
                .unwrap_or_default()
        )
    }
    let merchant = filters.q.clone();
    let (default_category, default_description) =
        state.work(move |s| s.merchant_default(&merchant)).await?;
    let previous = link(&filters, data.page.saturating_sub(1).max(1));
    let next = link(&filters, data.page + 1);
    render(Review {
        csrf: state.0.csrf.clone(),
        data,
        filters,
        previous,
        next,
        default_category,
        default_description,
    })
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
    for key in form.keys().filter_map(|k| k.strip_prefix("pick_")) {
        let (id, version) = key
            .split_once('_')
            .ok_or_else(|| Error::Invalid("Invalid selection.".into()))?;
        rows.push((
            id.parse()
                .map_err(|_| Error::Invalid("Invalid purchase ID.".into()))?,
            version
                .parse()
                .map_err(|_| Error::Invalid("Invalid decision version.".into()))?,
        ));
    }
    rows.sort_unstable();
    let change = match value("action").as_str() {
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
        _ => return Err(Error::Invalid("Choose a review action.".into())),
    };
    let token = state.work(move |s| s.stage_review(change)).await?;
    Ok(Redirect::to(&format!("/review/previews/{token}")))
}
async fn catalogue_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Redirect> {
    let mut csrf = None;
    let mut file = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| Error::Invalid("Invalid or oversized catalogue upload.".into()))?
    {
        match field.name() {
            Some("csrf") if csrf.is_none() => {
                csrf = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| Error::Invalid("Invalid form.".into()))?,
                )
            }
            Some("file") if file.is_none() => {
                let filename = field.file_name().unwrap_or("merchants.csv").to_owned();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| Error::Invalid("Invalid or oversized catalogue.".into()))?;
                if bytes.len() > MAX_UPLOAD {
                    return Err(Error::Invalid("CSV exceeds 10 MiB.".into()));
                }
                file = Some((filename, bytes.to_vec()));
            }
            _ => return Err(Error::Invalid("Unexpected catalogue field.".into())),
        }
    }
    if !csrf.is_some_and(|v| equal(&v, &state.0.csrf)) {
        return Err(Error::Invalid("Invalid CSRF token.".into()));
    }
    let (filename, bytes) = file.ok_or_else(|| Error::Invalid("Choose a merchant CSV.".into()))?;
    let token = state
        .work(move |s| {
            let change = Store::parse_catalogue(filename, bytes)?;
            s.stage_review(change)
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
    };
    match &page.data.change {
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
    state
        .work(move |s| s.commit_review(&token, &selected, allow))
        .await?;
    Ok(Redirect::to("/review"))
}
