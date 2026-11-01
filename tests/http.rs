use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use recon::{
    storage::Store,
    web::{AppState, router},
};
use tower::ServiceExt;

const FIRST: &str = include_str!("fixtures/virgin-first.csv");

#[tokio::test]
async fn execution_edit_and_history_routes_reject_untrusted_requests_without_echoing_credentials() {
    let (directory, app, cookie, csrf) = app().await;
    let token = "synthetic-private-credential";
    let forms = [
        ("/runs/start", format!("revision=0&token={token}")),
        ("/runs/1/resume", format!("token={token}")),
        ("/runs/1/notes/1", format!("token={token}&transaction=tx_test&confirmed=on")),
        ("/review/1/edit", "signature=stale&return_to=%2Freview&account=personal&category=Shopping&description=Test&action=stage".into()),
    ];
    for (path, fields) in forms {
        for protection in ["session", "origin", "csrf"] {
            let mut req = request(
                "POST",
                path,
                if protection == "session" { "" } else { &cookie },
                format!(
                    "csrf={}&{fields}",
                    if protection == "csrf" { "wrong" } else { &csrf }
                ),
                "application/x-www-form-urlencoded",
            );
            if protection == "origin" {
                req.headers_mut().remove("origin");
            }
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                response.status(),
                if protection == "csrf" {
                    StatusCode::BAD_REQUEST
                } else if protection == "session" {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::FORBIDDEN
                },
                "{path}: {protection}"
            );
            assert!(!text(response).await.contains(token));
        }
    }
    for protection in ["session", "origin", "csrf"] {
        let mut req = request(
            "POST",
            "/history/import",
            if protection == "session" { "" } else { &cookie },
            multipart(
                if protection == "csrf" { "wrong" } else { &csrf },
                "Reference,Account,Category,Description,Paid,Reason\n",
            ),
            "multipart/form-data; boundary=boundary",
        );
        if protection == "origin" {
            req.headers_mut().remove("origin");
        }
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            response.status(),
            if protection == "csrf" {
                StatusCode::BAD_REQUEST
            } else if protection == "session" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::FORBIDDEN
            }
        );
    }
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/runs/start",
            &cookie,
            format!("csrf={csrf}&revision=-1&token={token}"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(!text(response).await.contains(token));
    let db = rusqlite::Connection::open(directory.path().join("recon.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM recon_runs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM historical_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn request(
    method: &str,
    path: &str,
    cookie: &str,
    body: impl Into<Body>,
    content_type: &str,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:7310")
        .header("cookie", cookie);
    if method == "POST" {
        builder = builder.header("origin", "http://127.0.0.1:7310");
    }
    builder
        .header("content-type", content_type)
        .body(body.into())
        .unwrap()
}

async fn text(response: Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

async fn app() -> (tempfile::TempDir, Router, String, String) {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(Store::open(directory.path()).unwrap()).unwrap();
    let token = state.startup_url().split('#').nth(1).unwrap().to_owned();
    let app = router(state);
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/session",
            "",
            format!("token={token}"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let response = app
        .clone()
        .oneshot(request("GET", "/", &cookie, "", "text/html"))
        .await
        .unwrap();
    let html = text(response).await;
    let csrf = html
        .split("name=\"csrf\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    (directory, app, cookie, csrf)
}

fn multipart(csrf: &str, file: &str) -> String {
    format!(
        "--boundary\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n{csrf}\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"synthetic.csv\"\r\nContent-Type: text/csv\r\n\r\n{file}\r\n--boundary--\r\n"
    )
}

async fn upload(app: &Router, cookie: &str, csrf: &str, file: &str) -> Response {
    app.clone()
        .oneshot(request(
            "POST",
            "/imports",
            cookie,
            multipart(csrf, file),
            "multipart/form-data; boundary=boundary",
        ))
        .await
        .unwrap()
}

#[tokio::test]
async fn upload_review_fragments_and_normal_form_fallback() {
    let (_directory, app, cookie, csrf) = app().await;
    let response = upload(&app, &cookie, &csrf, FIRST).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let response = app
        .clone()
        .oneshot(request("GET", &location, &cookie, "", "text/html"))
        .await
        .unwrap();
    let html = text(response).await;
    assert!(html.contains("3 source records"));
    assert!(html.contains("View import results and exceptions"));
    let summary = text(
        app.clone()
            .oneshot(request("GET", "/imports/1", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(summary.contains("missing_reference"));
    let response = app
        .clone()
        .oneshot(request("GET", "/review", &cookie, "", "text/html"))
        .await
        .unwrap();
    let html = text(response).await;
    assert!(html.contains("2 distinct purchases"));
    assert!(html.contains("<!doctype html>"));
    let mut edit = request(
        "POST",
        "/review/1/account",
        &cookie,
        format!("csrf={csrf}&version=0&account=personal&page=1"),
        "application/x-www-form-urlencoded",
    );
    edit.headers_mut()
        .insert("hx-request", "true".parse().unwrap());
    let response = app.clone().oneshot(edit).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.starts_with("<section id=\"review-workspace\""));
    assert!(!html.contains("<!doctype html>"));
    assert!(html.contains("value=\"personal\" selected"));
    assert!(html.contains("name=\"version\" value=\"1\""));
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/1/account",
            &cookie,
            format!("csrf={csrf}&version=0&account=joint"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/1/account",
            &cookie,
            format!("csrf={csrf}&version=1&account=joint"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let response = upload(&app, &cookie, &csrf, FIRST).await;
    assert_eq!(response.headers()["location"], location);
}

#[tokio::test]
async fn host_origin_authentication_and_csrf_are_required() {
    let (_directory, app, cookie, csrf) = app().await;
    let response = app
        .clone()
        .oneshot(request("GET", "/review", "", "", "text/html"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!text(response).await.contains("SYNTHETIC"));
    for field in ["host", "origin"] {
        let mut req = request(
            "POST",
            "/imports",
            &cookie,
            multipart(&csrf, FIRST),
            "multipart/form-data; boundary=boundary",
        );
        req.headers_mut()
            .insert(field, "https://evil.invalid".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut req = request(
        "POST",
        "/imports",
        &cookie,
        multipart(&csrf, FIRST),
        "multipart/form-data; boundary=boundary",
    );
    req.headers_mut().remove("origin");
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        upload(&app, &cookie, "bad", FIRST).await.status(),
        StatusCode::BAD_REQUEST
    );
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/session",
            "",
            "token=bad",
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let html = text(
        app.clone()
            .oneshot(request("GET", "/", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("No imports yet"));
    upload(&app, &cookie, &csrf, FIRST).await;
    for form in [
        "csrf=bad&version=0&account=joint",
        "version=0&account=joint",
    ] {
        assert!(
            app.clone()
                .oneshot(request(
                    "POST",
                    "/review/1/account",
                    &cookie,
                    form,
                    "application/x-www-form-urlencoded"
                ))
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
    }
}

#[tokio::test]
async fn imported_html_and_filenames_are_escaped_and_security_headers_present() {
    let (_directory, app, cookie, csrf) = app().await;
    let evil = FIRST.replace(
        "SYNTHETIC SHOP",
        "<script>alert(1)</script><b hx-get='/evil'>test</b>",
    );
    assert_eq!(
        upload(&app, &cookie, &csrf, &evil).await.status(),
        StatusCode::SEE_OTHER
    );
    let response = app
        .clone()
        .oneshot(request("GET", "/review", &cookie, "", "text/html"))
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "same-origin");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        !response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("unsafe")
    );
    let html = text(response).await;
    assert!(!html.contains("<script>alert"));
    assert!(!html.contains("<b hx-get"));
    assert!(html.contains("&lt;script&gt;") || html.contains("&#60;script&#62;"));
}

#[tokio::test]
async fn oversized_upload_is_rejected_without_import() {
    let (_directory, app, cookie, csrf) = app().await;
    let response = upload(
        &app,
        &cookie,
        &csrf,
        &"x".repeat(recon::importer::MAX_UPLOAD + 1),
    )
    .await;
    assert!(response.status().is_client_error());
    let html = text(
        app.clone()
            .oneshot(request("GET", "/", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("No imports yet"));
}

#[tokio::test]
async fn pagination_fragments_and_not_found() {
    let (_directory, app, cookie, csrf) = app().await;
    let header = FIRST.lines().next().unwrap();
    let row = FIRST.lines().nth(1).unwrap();
    let mut csv = format!("{header}\n");
    for id in 1..=51 {
        csv.push_str(&row.replace("00000000000000000000001", &format!("{id:023}")));
        csv.push('\n');
    }
    upload(&app, &cookie, &csrf, &csv).await;
    let mut req = request("GET", "/review?page=2", &cookie, "", "text/html");
    req.headers_mut()
        .insert("hx-request", "true".parse().unwrap());
    let html = text(app.clone().oneshot(req).await.unwrap()).await;
    assert!(!html.contains("<!doctype html>"));
    assert!(html.contains("Page 2 of 2"));
    assert_eq!(html.matches("<tr id=\"review-").count(), 1);
    let response = app
        .clone()
        .oneshot(request("GET", "/imports/999", &cookie, "", "text/html"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn startup_landing_has_no_secrets_or_financial_data() {
    let (_directory, app, _cookie, _csrf) = app().await;
    let response = app
        .oneshot(request("GET", "/start", "", "", "text/html"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(!html.contains("SYNTHETIC"));
    assert!(!html.contains("csrf"));
    assert!(html.contains("/assets/app.js"));
}

#[tokio::test]
async fn old_purchase_bookmarks_redirect_to_the_single_review_workspace() {
    let (_directory, app, cookie, _csrf) = app().await;
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            "/purchases?page=2",
            &cookie,
            "",
            "text/html",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    let location = response.headers()["location"].to_str().unwrap();
    assert!(location.starts_with("/review?"));
    assert!(location.contains("page=2"));
    let html = text(
        app.clone()
            .oneshot(request("GET", "/review", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(!html.contains("Allocations"));
    assert!(!html.contains("href=\"/purchases"));
    assert!(html.contains("id=\"review-workspace\""));
}

#[tokio::test]
async fn inline_account_edits_refresh_filter_membership_counts_and_bulk_versions() {
    let (_directory, app, cookie, csrf) = app().await;
    upload(&app, &cookie, &csrf, FIRST).await;
    let path = "/review/1/account?q=SYNTHETIC%20SHOP&account=unset&missing=account&import_id=1&view=attention&page=1";
    let mut req = request(
        "POST",
        path,
        &cookie,
        format!("csrf={csrf}&version=0&account=personal"),
        "application/x-www-form-urlencoded",
    );
    req.headers_mut()
        .insert("hx-request", "true".parse().unwrap());
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains("1 distinct purchases"));
    assert!(html.contains("missing account 1"));
    assert!(!html.contains("id=\"review-1\""));
    assert!(html.contains("id=\"review-2\""));
    assert!(html.contains("form=\"bulk-review\" name=\"pick_2_0\""));
    assert!(html.contains("q=SYNTHETIC%20SHOP"));
    assert!(html.contains("import_id=1"));
    assert!(
        !html.contains("id=\"bulk-review\""),
        "unrelated draft controls stay outside the swap"
    );
    assert!(html.contains("hx-target=\"#review-workspace\""));
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/1/account?q=SYNTHETIC%20SHOP&import_id=1",
            &cookie,
            format!("csrf={csrf}&version=1&account=joint"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap();
    assert!(location.starts_with("/review?"));
    assert!(location.contains("q=SYNTHETIC%20SHOP"));
    assert!(location.contains("import_id=1"));
    let html = text(
        app.clone()
            .oneshot(request("GET", location, &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("name=\"pick_1_2\""));
    assert!(html.contains("value=\"joint\" selected"));
}

#[tokio::test]
async fn invalid_return_filters_cannot_commit_an_inline_edit() {
    let (_directory, app, cookie, csrf) = app().await;
    upload(&app, &cookie, &csrf, FIRST).await;
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/1/account?account=invalid",
            &cookie,
            format!("csrf={csrf}&version=0&account=personal"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let html = text(
        app.clone()
            .oneshot(request("GET", "/review", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("name=\"pick_1_0\""));
    assert!(html.contains("missing account 2"));
}

#[tokio::test]
async fn merchant_review_preview_commit_filters_and_security() {
    let (_directory, app, cookie, csrf) = app().await;
    upload(&app, &cookie, &csrf, FIRST).await;
    for path in [
        "/review",
        "/review?q=&account=&missing=&view=attention&import_id=",
    ] {
        let response = app
            .clone()
            .oneshot(request("GET", path, &cookie, "", "text/html"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let html = text(response).await;
        assert!(html.contains("2 distinct purchases"));
        assert!(html.contains("confirm what still needs funding"));
    }
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/review", "", "", "text/html"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                "/review/stage",
                &cookie,
                "csrf=bad&action=default&merchant=Test",
                "application/x-www-form-urlencoded"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let response=app.clone().oneshot(request("POST","/review/stage",&cookie,format!("csrf={csrf}&action=default&merchant=SYNTHETIC+SHOP&category=Shopping&description=%3Cscript%3Etest%3C%2Fscript%3E"),"application/x-www-form-urlencoded")).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let html = text(
        app.clone()
            .oneshot(request("GET", &location, &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("2 potentially affected purchases"));
    assert!(!html.contains("<script>test</script>"));
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("{location}/commit"),
            &cookie,
            format!("csrf={csrf}&allow_categories=on"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let html = text(
        app.clone()
            .oneshot(request(
                "GET",
                "/review?missing=category",
                &cookie,
                "",
                "text/html",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("0 distinct purchases"));
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                &format!("{location}/commit"),
                &cookie,
                format!("csrf={csrf}"),
                "application/x-www-form-urlencoded"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let catalogue = "Merchant,Category,Description\nSYNTHETIC SHOP,Shopping,First\n synthetic shop ,Shopping,Second\n";
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/catalogue",
            &cookie,
            multipart(&csrf, catalogue),
            "multipart/form-data; boundary=boundary",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let html = text(
        app.clone()
            .oneshot(request("GET", &location, &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("Conflict: explicitly select"));
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                &format!("{location}/commit"),
                &cookie,
                format!("csrf={csrf}&record_2=on&record_3=on"),
                "application/x-www-form-urlencoded"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                &format!("{location}/commit"),
                &cookie,
                format!("csrf={csrf}&record_3=on"),
                "application/x-www-form-urlencoded"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn bulk_review_http_stale_preview_and_selection_bounds() {
    let (_directory, app, cookie, csrf) = app().await;
    upload(&app, &cookie, &csrf, FIRST).await;
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/stage",
            &cookie,
            format!("csrf={csrf}&action=accounts&account=joint&pick_1_0=on&pick_2_0=on"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    app.clone()
        .oneshot(request(
            "POST",
            "/review/2/account",
            &cookie,
            format!("csrf={csrf}&version=0&account=personal"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                &format!("{location}/commit"),
                &cookie,
                format!("csrf={csrf}"),
                "application/x-www-form-urlencoded"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let html = text(
        app.clone()
            .oneshot(request(
                "GET",
                "/review?account=unset",
                &cookie,
                "",
                "text/html",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("1 distinct purchases"));
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                "/review/stage",
                &cookie,
                format!("csrf={csrf}&action=accounts&account=joint"),
                "application/x-www-form-urlencoded"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    for path in [
        "/review/stage",
        "/review/catalogue",
        "/review/previews/invalid/commit",
    ] {
        let mut req = request(
            "POST",
            path,
            &cookie,
            "",
            "application/x-www-form-urlencoded",
        );
        req.headers_mut().remove("origin");
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
}

async fn stage_and_confirm(app: &Router, cookie: &str, csrf: &str, fields: &str) -> String {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/stage",
            cookie,
            format!("csrf={csrf}&{fields}"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let preview = app
        .clone()
        .oneshot(request("GET", &location, cookie, "", "text/html"))
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::OK);
    let html = text(preview).await;
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("{location}/commit"),
            cookie,
            format!("csrf={csrf}&allow_categories=on"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    html
}

#[tokio::test]
async fn global_queue_staging_external_handling_totals_and_history_workflow() {
    let (_directory, app, cookie, csrf) = app().await;
    upload(&app, &cookie, &csrf, FIRST).await;
    let html = text(
        app.clone()
            .oneshot(request("GET", "/runs", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("No recon runs yet"));
    stage_and_confirm(&app,&cookie,&csrf,"action=default&merchant=SYNTHETIC%20SHOP&category=Shopping&description=Synthetic%20purchase").await;
    stage_and_confirm(
        &app,
        &cookie,
        &csrf,
        "action=accounts&account=joint&pick_1_0=on&pick_2_0=on",
    )
    .await;
    stage_and_confirm(&app,&cookie,&csrf,"action=handling&handling=external&pick_1_1=on&reason=Partner%20already%20funded%20the%20pot").await;
    let page = text(
        app.clone()
            .oneshot(request("GET", "/review", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    let signature = page
        .split("name=\"pick_2_1\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    stage_and_confirm(
        &app,
        &cookie,
        &csrf,
        &format!("action=stage&pick_2_1={signature}"),
    )
    .await;
    let html = text(
        app.clone()
            .oneshot(request("GET", "/run", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("£12.34"));
    assert!(html.contains("1 source exceptions"));
    assert!(!html.contains("name=\"token\""));
    stage_and_confirm(
        &app,
        &cookie,
        &csrf,
        "action=exception&observation_id=3&reason=Synthetic+non-funding+row",
    )
    .await;
    let ready = text(
        app.clone()
            .oneshot(request("GET", "/run", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(ready.contains("Confirm and run recon"));
    let html = text(
        app.clone()
            .oneshot(request(
                "GET",
                "/review?view=staged",
                &cookie,
                "",
                "text/html",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("1 distinct purchases"));
    assert!(html.contains("id=\"review-2\""));
    assert!(!html.contains("id=\"review-1\""));
    assert!(!html.contains("Target recon"));
    let html = text(
        app.clone()
            .oneshot(request(
                "GET",
                "/review/1/history",
                &cookie,
                "",
                "text/html",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("external v1"));
    assert!(html.contains("Partner already funded the pot"));
    assert!(html.contains("joint v1"));
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/review/stage",
            &cookie,
            format!("csrf={csrf}&action=handling&handling=outstanding&pick_1_1=on&reason=Reopen"),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let preview = stage_and_confirm(&app,&cookie,&csrf,"action=handling&handling=outstanding&pick_1_1=on&reason=Confirmed%20original%20transfer%20was%20reversed&reopen=on").await;
    assert!(preview.contains("Explicit reopening was requested"));
    let html = text(
        app.clone()
            .oneshot(request("GET", "/run", &cookie, "", "text/html"))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("1 purchases need review"));
    assert!(!html.contains("name=\"token\""));
}

#[tokio::test]
async fn recon_routes_and_handling_mutations_require_authentication_origin_and_csrf() {
    let (_directory, app, cookie, csrf) = app().await;
    for path in [
        "/recons",
        "/recons/1",
        "/review/1/history",
        "/run",
        "/runs",
        "/rules",
        "/merchants",
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", path, "", "", "text/html"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    upload(&app, &cookie, &csrf, FIRST).await;
    for fields in [
        "csrf=bad&action=create_recon&name=Denied",
        "action=handling&handling=external&pick_1_0=on&reason=Denied",
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "POST",
                    "/review/stage",
                    &cookie,
                    fields,
                    "application/x-www-form-urlencoded"
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut req = request(
        "POST",
        "/review/stage",
        &cookie,
        format!("csrf={csrf}&action=handling&handling=external&pick_1_0=on&reason=Denied"),
        "application/x-www-form-urlencoded",
    );
    req.headers_mut()
        .insert("origin", "https://evil.invalid".parse().unwrap());
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let html = text(
        app.clone()
            .oneshot(request(
                "GET",
                "/review/1/history",
                &cookie,
                "",
                "text/html",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("No decisions yet; handling is unknown"));
    assert_eq!(
        app.clone()
            .oneshot(request("GET", "/recons/999", &cookie, "", "text/html"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn rule_names_reasons_and_history_are_escaped() {
    let (_directory, app, cookie, csrf) = app().await;
    upload(&app, &cookie, &csrf, FIRST).await;
    stage_and_confirm(
        &app,
        &cookie,
        &csrf,
        "action=account_rule&merchant=%3Cscript%3Eevil%3C%2Fscript%3E&account=joint",
    )
    .await;
    stage_and_confirm(
        &app,
        &cookie,
        &csrf,
        "action=handling&handling=external&pick_1_0=on&reason=%3Cb%3Enot%20markup%3C%2Fb%3E",
    )
    .await;
    for path in ["/rules", "/review?view=all", "/review/1/history"] {
        let html = text(
            app.clone()
                .oneshot(request("GET", path, &cookie, "", "text/html"))
                .await
                .unwrap(),
        )
        .await;
        assert!(!html.contains("<script>evil"));
        assert!(!html.contains("<b>not markup"));
    }
}
