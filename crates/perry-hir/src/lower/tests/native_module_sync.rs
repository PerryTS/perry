#[test]
fn direct_named_calls_use_the_synchronized_esm_cell() {
    let source = r#"
import { readFile } from "node:fs";
import { syncBuiltinESMExports } from "node:module";
syncBuiltinESMExports();
readFile();
"#;
    let module = perry_parser::parse_typescript(source, "sync-builtins.ts").expect("source parses");
    let hir = super::super::lower_module(&module, "sync-builtins", "sync-builtins.ts")
        .expect("source lowers");
    let dump = format!("{:#?}", hir.init);
    assert!(
        dump.contains("js_native_module_named_esm_export_value"),
        "named call must read the synchronized ESM cell: {dump}"
    );
    assert!(
        dump.matches("js_native_module_named_esm_export_value")
            .count()
            >= 2,
        "named import must initialize its ESM cell before the dynamic call: {dump}"
    );
    assert!(
        !dump.contains("module: \"fs\",\n                class_name: None,\n                object: None,\n                method: \"readFile\""),
        "named call must not bypass the ESM cell through native dispatch: {dump}"
    );
}

#[test]
fn callee_only_builtin_import_does_not_materialize_an_export_cell() {
    for source in [
        r#"import { readFileSync } from 'fs'; console.log(readFileSync('x', 'utf8'));"#,
        r#"import { readFileSync as read } from 'node:fs'; console.log(read('x', 'utf8'));"#,
    ] {
        let ast = perry_parser::parse_typescript(source, "direct.ts").unwrap();
        let hir = super::super::lower_module(&ast, "direct", "direct.ts").unwrap();
        let dump = format!("{:?}", hir.init);
        assert!(dump.contains("readFileSync"), "call must remain: {dump}");
        assert!(
            !dump.contains("js_native_module_named_esm_export_value"),
            "unused cell: {dump}"
        );
    }
}

#[test]
fn escaped_builtin_import_keeps_its_initial_binding() {
    let source = r#"import { readFileSync } from 'fs'; const f = readFileSync; console.log(f);"#;
    let ast = perry_parser::parse_typescript(source, "value.ts").unwrap();
    let hir = super::super::lower_module(&ast, "value", "value.ts").unwrap();
    let dump = format!("{:?}", hir.init);
    assert_eq!(
        dump.matches("js_native_module_named_esm_export_value")
            .count(),
        2
    );
}

#[test]
fn native_stream_heritage_keeps_its_import_cell() {
    let source =
        r#"import { Readable as Base } from 'node:stream'; class Body extends Base {} new Body();"#;
    let ast = perry_parser::parse_typescript(source, "heritage.ts").unwrap();
    let hir = super::super::lower_module(&ast, "heritage", "heritage.ts").unwrap();
    assert!(hir.classes.iter().any(|c| c
        .native_extends
        .as_ref()
        .is_some_and(|p| p.0 == "node_stream")));
    let dump = format!("{:?}", hir.init);
    assert!(
        dump.contains("js_native_module_named_esm_export_value"),
        "missing constructor initialization: {dump}"
    );
}

#[test]
fn named_constructor_apply_is_a_value_call() {
    let source = r#"import { Writable } from 'node:stream'; Writable.apply({}, []);"#;
    let ast = perry_parser::parse_typescript(source, "apply.ts").unwrap();
    let hir = super::super::lower_module(&ast, "apply", "apply.ts").unwrap();
    let dump = format!("{:?}", hir.init);
    assert!(
        dump.contains("js_native_module_named_esm_export_value"),
        "{dump}"
    );
    assert!(!dump.contains("stream.apply is not implemented"), "{dump}");
}
