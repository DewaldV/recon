use std::path::PathBuf;

use clap::Parser;
use recon::{
    config,
    storage::Store,
    web::{AppState, router},
};

#[derive(Parser)]
#[command(about = "Local, offline Virgin credit-card reconciliation. No Monzo execution.")]
struct Args {
    /// Private local directory outside the repository and cloud-sync folders.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Write a verified SQLite snapshot and exit, with the app otherwise stopped.
    #[arg(long)]
    backup: bool,
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
    let store = tokio::task::spawn_blocking(move || Store::open(&directory))
        .await
        .map_err(|_| recon::Error::Worker)??;
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
        "Recon is running locally. No money movement is available.\nOpen this private startup link in your browser:\n{}",
        state.startup_url()
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
