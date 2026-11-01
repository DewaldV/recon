pub mod config;
pub mod cutover;
pub mod execution;
pub mod history_import;
pub mod import_repair;
pub mod importer;
pub mod monzo;
pub mod queue;
pub mod recon;
pub mod review;
pub mod rules;
pub mod settings;
pub mod storage;
pub mod web;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("That record was not found.")]
    NotFound,
    #[error("This decision changed in another tab. Reload before editing again.")]
    Stale,
    #[error("Database unavailable or busy. Retry shortly; no partial changes were saved.")]
    Database(#[from] rusqlite::Error),
    #[error("Local storage unavailable. Check the data directory and permissions.")]
    Io(#[from] std::io::Error),
    #[error("The local worker is unavailable. Restart the app.")]
    Worker,
}

pub type Result<T> = std::result::Result<T, Error>;
