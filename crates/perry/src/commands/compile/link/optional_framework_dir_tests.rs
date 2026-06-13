use super::*;

/// Lay out a temp project: `<root>/perry.toml` + `<root>/src/main.ts`,
/// with the perry.toml `[google_auth]` table set to `toml_body`.
/// Returns (tempdir, entry-ts-path).
fn scaffold(toml_body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("perry.toml"), toml_body).unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let entry = src.join("main.ts");
    fs::write(&entry, "export {}\n").unwrap();
    (dir, entry)
}

#[test]
fn resolves_framework_dir_relative_to_project_root() {
    let (dir, entry) =
        scaffold("[google_auth]\nframework_dir = \"vendor/google-sign-in/frameworks\"\n");
    // Use a uniquely-named env var that is guaranteed unset.
    let env_name = "PERRY_TEST_GA_FRAMEWORK_DIR_UNSET_A";
    let resolved = resolve_optional_framework_dir(env_name, &entry).unwrap();
    // Compare against the canonicalized root — `find_project_root_for`
    // canonicalizes the entry, so the resolved path is symlink-resolved
    // (e.g. /var/folders → /private/var on macOS).
    assert_eq!(
        resolved,
        dir.path()
            .canonicalize()
            .unwrap()
            .join("vendor/google-sign-in/frameworks")
    );
}

#[test]
fn returns_none_when_no_framework_dir_key() {
    let (_dir, entry) = scaffold("[google_auth]\nios_client_id = \"abc\"\n");
    let env_name = "PERRY_TEST_GA_FRAMEWORK_DIR_UNSET_B";
    assert!(resolve_optional_framework_dir(env_name, &entry).is_none());
}

#[test]
fn env_var_takes_precedence_over_perry_toml() {
    let (_dir, entry) = scaffold("[google_auth]\nframework_dir = \"vendor/from-toml\"\n");
    // Unique name so we don't race other tests sharing process env.
    let env_name = "PERRY_TEST_GA_FRAMEWORK_DIR_SET_C";
    std::env::set_var(env_name, "/absolute/from/env");
    let resolved = resolve_optional_framework_dir(env_name, &entry).unwrap();
    std::env::remove_var(env_name);
    assert_eq!(resolved, PathBuf::from("/absolute/from/env"));
}
