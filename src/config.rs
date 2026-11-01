use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

use crate::{Error, Result};

pub fn random_secret() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Invalid("Operating system randomness unavailable.".into()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn data_directory(override_path: Option<PathBuf>) -> Result<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Invalid("HOME must be set.".into()))?;
    let path = override_path.unwrap_or_else(|| {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("recon")
    });
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::Invalid(
            "Data directory must be absolute, without '..'.".into(),
        ));
    }
    let mut forbidden = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        home.join("Projects"),
        home.join("Documents/financial/recons"),
    ];
    for name in ["ProtonDrive", "Proton Drive", "Dropbox", "OneDrive"] {
        forbidden.push(home.join(name));
    }
    if let Some(roots) = env::var_os("RECON_SYNC_ROOTS") {
        forbidden.extend(env::split_paths(&roots));
    }
    let resolved = resolve_existing_ancestors(&path)?;
    for root in forbidden {
        if resolved.starts_with(resolve_existing_ancestors(&root)?) {
            return Err(Error::Invalid("Data directory cannot be inside source, legacy recon, or configured cloud-sync folders.".into()));
        }
    }
    private_directory(&resolved)?;
    Ok(resolved)
}

fn resolve_existing_ancestors(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        // A dangling symlink must not be treated as a missing directory.
        if fs::symlink_metadata(ancestor).is_ok() {
            return Err(Error::Invalid(
                "Data path contains a dangling symlink.".into(),
            ));
        }
        suffix.push(
            ancestor
                .file_name()
                .ok_or_else(|| Error::Invalid("Invalid data path.".into()))?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid data path.".into()))?;
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

pub fn private_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(Error::Invalid(
            "Data path must be a regular directory.".into(),
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn private_file(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path);
    let file = match file {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(path)?.is_file() {
                return Err(Error::Invalid(
                    "Database/lock must be a regular file, not a symlink.".into(),
                ));
            }
            fs::OpenOptions::new().read(true).write(true).open(path)?
        }
        Err(e) => return Err(e.into()),
    };
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
