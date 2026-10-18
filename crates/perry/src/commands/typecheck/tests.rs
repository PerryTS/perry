use super::*;

#[test]
fn native_type_flags_do_not_use_tsc_enum_values() {
    for (flags, text, expected) in [
        (4, "undefined", Type::Void),
        (8, "null", Type::Null),
        (16, "void", Type::Void),
        (32, "string", Type::String),
        (64, "number", Type::Number),
        (128, "bigint", Type::BigInt),
        (256, "boolean", Type::Boolean),
        (512, "symbol", Type::Symbol),
        (1024, "\"hello\"", Type::String),
        (2048, "123", Type::Number),
        (4096, "123n", Type::BigInt),
        (8192, "true", Type::Boolean),
        (262144, "never", Type::Never),
    ] {
        assert_eq!(ts_flags_to_perry_type(flags, text), expected, "{text}");
    }
}

// The preview RPC API is unversioned. These opt-in tests require the exact
// npm build on PATH or symlinked as node_modules/.bin/tsgo in the temp project.
#[test]
#[ignore = "set TSGO_TEST_BINARY to native-preview 7.0.0-dev.20260612.1"]
fn real_tsgo_snapshot_handles_types_and_utf8_positions() {
    let binary = std::env::var_os("TSGO_TEST_BINARY").expect("TSGO_TEST_BINARY required");
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("node_modules/.bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&binary, bin_dir.join("tsgo")).unwrap();
    #[cfg(not(unix))]
    std::fs::copy(&binary, bin_dir.join("tsgo.exe")).unwrap();
    let config = dir.path().join("tsconfig.json");
    std::fs::write(
        &config,
        r#"{"compilerOptions":{"strict":true,"noEmit":true,"target":"ES2022"}}"#,
    )
    .unwrap();
    let source = "// 😀é\nexport const value = 42; export const text = 'hello';\n";
    let file = dir.path().join("main.ts");
    std::fs::write(&file, source).unwrap();
    let mut client = TsGoClient::spawn(dir.path()).unwrap();
    client.load_project(&config).unwrap();
    assert!(client.diagnostics().unwrap().is_empty());
    let mut params = client.project_params().unwrap();
    params["file"] = file.to_str().unwrap().into();
    let ast = client
        .send_request_binary("getSourceFile", &params)
        .unwrap();
    assert!(ast.len() > 64, "binary source-file AST should be present");
    let values = client
        .get_types_at_positions(
            file.to_str().unwrap(),
            &[
                source.find("value").unwrap() as u32,
                source.find("text").unwrap() as u32,
            ],
        )
        .unwrap();
    let value = values[0].as_ref().unwrap();
    assert_eq!(value.type_string, "42");
    assert_eq!(
        ts_flags_to_perry_type(value.flags, &value.type_string),
        Type::Number
    );
    let text = values[1].as_ref().unwrap();
    assert_eq!(
        ts_flags_to_perry_type(text.flags, &text.type_string),
        Type::String
    );
    let mut cache = perry_diagnostics::SourceCache::new();
    let parsed = perry_parser::parse_typescript_with_cache(source, "main.ts", &mut cache).unwrap();
    let positions = collect_untyped_positions(&parsed.module);
    let resolved = resolve_types_for_file(&mut client, file.to_str().unwrap(), &positions).unwrap();
    assert_eq!(resolved.get(&positions[0].0), Some(&Type::Number));
    assert_eq!(resolved.get(&positions[1].0), Some(&Type::String));
    let error = client
        .send_request("notARealMethod", &serde_json::Value::Null)
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ApiError>(),
        Some(ApiError::Remote { .. })
    ));
}
