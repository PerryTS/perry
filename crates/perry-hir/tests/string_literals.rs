use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, ImportSpecifier, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "string_literals.ts", &mut cache)
        .expect("parse should succeed");
    lower_module(&parsed.module, "test", "string_literals.ts").expect("lower should succeed")
}

#[test]
fn non_ascii_string_literal_stays_utf8() {
    let module = lower_src(r#"const value = "héllo 世界 🌍";"#);
    let init = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(expr),
                ..
            } if name == "value" => Some(expr),
            _ => None,
        })
        .expect("value binding should be lowered");

    assert!(
        matches!(init, Expr::String(value) if value == "héllo 世界 🌍"),
        "{init:?}"
    );
}

#[test]
fn node_submodule_default_import_remains_default_specifier() {
    let module = lower_src(
        r#"
        import consumersDefault, { text } from "node:stream/consumers";
        import * as consumers from "node:stream/consumers";
        console.log(consumersDefault, consumers, text);
        "#,
    );

    let mixed = module
        .imports
        .iter()
        .find(|import| import.specifiers.len() == 2)
        .expect("mixed default/named import should be lowered");
    assert!(
        mixed.specifiers.iter().any(|specifier| matches!(
            specifier,
            ImportSpecifier::Default { local } if local == "consumersDefault"
        )),
        "{:?}",
        mixed.specifiers
    );
    assert!(
        mixed.specifiers.iter().any(|specifier| matches!(
            specifier,
            ImportSpecifier::Named { imported, local } if imported == "text" && local == "text"
        )),
        "{:?}",
        mixed.specifiers
    );
    assert!(
        !mixed.specifiers.iter().any(|specifier| matches!(
            specifier,
            ImportSpecifier::Namespace { local } if local == "consumersDefault"
        )),
        "{:?}",
        mixed.specifiers
    );
}
