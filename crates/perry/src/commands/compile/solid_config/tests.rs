use super::*;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn jsx_runtime_follows_nearest_jsonc_config_and_extends_without_crossing_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("tsconfig.base.json"),
        r#"{"compilerOptions":{"jsxImportSource":"@opentui/solid"}}"#,
    );
    write(
        &dir.path().join("tsconfig.json"),
        "{ // inherited JSX\n\"extends\": \"./tsconfig.base\", }",
    );
    let entry = dir.path().join("src/main.tsx");
    assert_eq!(
        JsxMode::Auto.runtime_for(&entry).unwrap().as_deref(),
        Some("@opentui/solid")
    );
    let (_, inputs) = crate::commands::compile::resolve::tsconfig_paths::jsx_config(&entry);
    assert!(inputs.contains(&dir.path().join("tsconfig.base.json")));
    write(
        &dir.path().join("src/react/tsconfig.json"),
        r#"{"compilerOptions":{"jsxImportSource":"react"}}"#,
    );
    assert!(JsxMode::Auto
        .runtime_for(&dir.path().join("src/react/main.tsx"))
        .unwrap()
        .is_none());
    assert!(JsxMode::Auto
        .runtime_for(&dir.path().join("node_modules/widget/main.tsx"))
        .unwrap()
        .is_none());
    // A watch/rebuild must reread the file, not reuse a process-global config.
    write(
        &dir.path().join("tsconfig.base.json"),
        r#"{"compilerOptions":{"jsxImportSource":"react"}}"#,
    );
    assert!(JsxMode::Auto.runtime_for(&entry).unwrap().is_none());
}

#[test]
fn explicit_runtime_and_default_override_automatic_detection() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.tsx");
    write(
        &dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"jsxImportSource":"@opentui/solid"}}"#,
    );
    for (value, expected) in [
        (serde_json::json!("solid"), Some("perry-solid")),
        (serde_json::json!("default"), None),
        (
            serde_json::json!({"runtime":"custom-renderer"}),
            Some("custom-renderer"),
        ),
    ] {
        assert_eq!(
            JsxMode::parse(&value)
                .unwrap()
                .runtime_for(&entry)
                .unwrap()
                .as_deref(),
            expected
        );
        write(
            &dir.path().join("package.json"),
            &serde_json::json!({"perry":{"jsx": value}}).to_string(),
        );
        assert_eq!(
            JsxMode::Auto.runtime_for(&entry).unwrap().as_deref(),
            expected
        );
    }
    for invalid in [
        serde_json::json!({"runtime":""}),
        serde_json::json!({"runtime":true}),
        serde_json::json!(true),
        serde_json::json!("soldi"),
    ] {
        assert!(JsxMode::parse(&invalid).is_err());
    }
}
