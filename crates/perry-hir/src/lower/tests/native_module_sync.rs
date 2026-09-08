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
