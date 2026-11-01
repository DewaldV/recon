use std::path::PathBuf;

use clap::Parser;
use recon::{
    config,
    storage::Store,
    web::{AppState, router},
};

#[derive(Parser)]
#[command(about = "Local Virgin credit-card review, staging, and resumable Monzo reconciliation.")]
struct Args {
    /// Private local directory outside the repository and cloud-sync folders.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Write a verified SQLite snapshot and exit, with the app otherwise stopped.
    #[arg(long)]
    backup: bool,
    /// Set a permanent inclusive posting-date start boundary and exit (app must be stopped).
    #[arg(
        long,
        value_name = "YYYY-MM-DD",
        requires = "cutover_reason",
        conflicts_with = "backup"
    )]
    start_date: Option<String>,
    /// Evidence that earlier purchases belong to historical reconciliation.
    #[arg(long, requires = "start_date")]
    cutover_reason: Option<String>,
    /// Retry validated CSV repairs for an archived import, then exit (app stopped).
    #[arg(long, value_name = "IMPORT_ID", conflicts_with_all = ["backup", "start_date"])]
    repair_import: Option<i64>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Recon: {error}");
        std::process::exit(1);
    }
}

async fn run() -> recon::Result<()> {
    let args = Args::parse();
    let directory = config::data_directory(args.data_dir)?;
    let mut store = tokio::task::spawn_blocking(move || Store::open(&directory))
        .await
        .map_err(|_| recon::Error::Worker)??;
    if let Some(id) = args.repair_import {
        let result = tokio::task::spawn_blocking(move || store.repair_import(id))
            .await
            .map_err(|_| recon::Error::Worker)??;
        if let Some(path) = result.backup {
            println!("Verified backup: {}", path.display());
        }
        println!("Recovered {} quarantined source rows.", result.repaired);
        return Ok(());
    }
    if let Some(date) = args.start_date {
        let reason = args.cutover_reason.expect("clap requires cutover evidence");
        let result = tokio::task::spawn_blocking(move || store.set_cutover(&date, &reason))
            .await
            .map_err(|_| recon::Error::Worker)??;
        println!(
            "Verified backup: {}\nExcluded {} pre-cutover purchases; resolved {} pre-cutover source exceptions.",
            result.backup.display(),
            result.excluded,
            result.resolved_exceptions
        );
        return Ok(());
    }
    if args.backup {
        let path = tokio::task::spawn_blocking(move || store.backup())
            .await
            .map_err(|_| recon::Error::Worker)??;
        println!("Verified backup: {}", path.display());
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:7310")
        .await
        .map_err(|_| {
            recon::Error::Invalid(
                "Cannot bind 127.0.0.1:7310. Stop the other process using this port.".into(),
            )
        })?;
    let state = AppState::new(store)?;
    println!(
        "Recon is running locally. Review is offline; execution needs a temporary Monzo token.\nOpen this private startup link in your browser:\n{}",
        state.startup_url()
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
