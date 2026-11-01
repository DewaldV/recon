use recon::config::data_directory;

#[test]
fn rejects_relative_parent_and_source_paths() {
    for path in ["relative", "/tmp/../escape", env!("CARGO_MANIFEST_DIR")] {
        assert!(data_directory(Some(path.into())).is_err());
    }
}

#[test]
fn canonicalizes_ancestors_before_source_path_checks() {
    let directory = tempfile::tempdir().unwrap();
    let link = directory.path().join("link");
    std::os::unix::fs::symlink(env!("CARGO_MANIFEST_DIR"), &link).unwrap();
    assert!(data_directory(Some(link.join("must-not-be-created"))).is_err());
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("must-not-be-created")
            .exists()
    );
}

#[test]
fn accepts_private_local_directory_outside_repository() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("local-data");
    assert_eq!(data_directory(Some(data.clone())).unwrap(), data);
}
