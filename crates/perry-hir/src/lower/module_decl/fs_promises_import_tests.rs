use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(source: String) -> Module {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &source,
                "fs_promises_import.ts",
                &mut cache,
            )
            .unwrap();
            crate::lower_module(&parsed.module, "test", "fs_promises_import.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn named_fs_promises_reads_the_parent_named_export_cell() {
    for source in ["fs", "node:fs"] {
        let module = lower(format!(
            "import {{ promises as files }} from {source:?}; export function value() {{ return files; }}"
        ));
        let hir = format!("{module:?}");
        assert!(
            hir.contains("js_native_module_named_esm_export_value"),
            "named object imports must retain the parent export cell: {hir}"
        );
        assert!(
            !hir.contains("NativeModuleRef(\"fs/promises\")"),
            "the named CommonJS object is not the fs/promises ESM namespace: {hir}"
        );
        assert!(
            hir.contains("String(\"fs\"), String(\"promises\")"),
            "{hir}"
        );
        let value = module
            .functions
            .iter()
            .find(|function| function.name == "value")
            .unwrap();
        let body = format!("{:?}", value.body);
        assert!(
            body.contains("js_native_module_named_esm_export_value"),
            "{body}"
        );
        assert!(
            body.contains("String(\"fs\"), String(\"promises\")"),
            "{body}"
        );
    }
}

#[test]
fn parent_export_snapshot_is_initialized_before_user_mutation() {
    let module = lower(
        "import fs, { promises } from 'node:fs'; Object.defineProperty(fs, 'promises', { value: {} }); export function value() { return promises; }".to_string(),
    );
    let first = format!("{:?}", module.init.first().unwrap());
    assert!(
        first.contains("js_native_module_named_esm_export_value"),
        "{first}"
    );
    assert!(
        first.contains("String(\"fs\"), String(\"promises\")"),
        "{first}"
    );
}

#[test]
fn other_native_promises_keep_their_existing_submodule_routes() {
    for source in ["dns", "stream", "readline"] {
        let module = lower(format!(
            "import {{ promises as child }} from {source:?}; export function value() {{ return child; }}"
        ));
        let hir = format!("{module:?}");
        assert!(
            hir.contains(&format!("NativeModuleRef(\"{source}/promises\")")),
            "{hir}"
        );
    }
}

#[test]
fn promises_member_calls_do_not_become_callback_fs_calls() {
    for source in ["fs", "node:fs"] {
        let module = lower(format!(
            "import {{ promises as files }} from {source:?}; export function read(path: string) {{ return files.readFile(path, 'utf8'); }}"
        ));
        let hir = format!("{module:?}");
        assert!(
            hir.contains("readFile"),
            "the read must remain reachable: {hir}"
        );
        assert!(
            !hir.contains("NativeMethodCall { module: \"fs\", method: \"readFile\""),
            "promise reads must not use the callback fs bucket: {hir}"
        );
        let read = module
            .functions
            .iter()
            .find(|function| function.name == "read")
            .unwrap();
        let body = format!("{:?}", read.body);
        assert!(
            body.contains("js_native_module_named_esm_export_value"),
            "{body}"
        );
        assert!(
            body.contains("String(\"fs\"), String(\"promises\")"),
            "{body}"
        );
    }
}
