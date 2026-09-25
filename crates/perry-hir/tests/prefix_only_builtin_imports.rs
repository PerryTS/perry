use perry_hir::{lower_module, Module};
use perry_parser::parse_typescript;

fn lower(source: &str) -> Module {
    let parsed = parse_typescript(source, "prefix-only.ts").unwrap();
    lower_module(&parsed, "prefix_only", "prefix-only.ts").unwrap()
}

#[test]
fn bare_prefix_only_names_are_source_module_imports() {
    for name in ["sea", "sqlite", "test", "test/reporters"] {
        for binding in ["* as value", "value", "{ marker as value }"] {
            let module = lower(&format!(
                "import {binding} from '{name}'; console.log(value);"
            ));
            assert_eq!(module.imports.len(), 1);
            assert!(!module.imports[0].is_native, "{binding} from {name}");
            assert_eq!(module.imports[0].source, name);
        }
    }
}

#[test]
fn explicit_node_prefix_survives_until_driver_resolution() {
    for name in ["sea", "sqlite", "test", "test/reporters"] {
        for binding in ["* as value", "value"] {
            let specifier = format!("node:{name}");
            let module = lower(&format!(
                "import {binding} from '{specifier}'; console.log(value);"
            ));
            assert_eq!(module.imports.len(), 1);
            assert_eq!(module.imports[0].source, specifier);
            if matches!(name, "sea" | "sqlite") {
                assert!(module.imports[0].is_native);
            }
        }
    }
    let module = lower("import * as value from 'node:fs'; console.log(value);");
    assert!(module.imports[0].is_native);
    assert_eq!(module.imports[0].source, "fs");
}
