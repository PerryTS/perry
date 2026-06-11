//! Issue #4976: inline `export default class Name { … }` must lower the
//! class body, not just record the export name. Pre-fix, the
//! `DefaultDecl::Class` arm dropped the class entirely — the importer's
//! `exported_classes` lookup missed and `new Widget()` fell through to the
//! empty-object placeholder with every prototype method and field
//! initializer gone (ink's `export default class Ink { render() {…} }`).

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Export, Module};
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "export_default_class.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "export_default_class.ts")
                .map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn has_named_export(module: &Module, local: &str, exported: &str) -> bool {
    module.exports.iter().any(
        |e| matches!(e, Export::Named { local: l, exported: x } if l == local && x == exported),
    )
}

#[test]
fn named_inline_export_default_class_lowers_full_body() {
    let module = lower_result(
        r#"
        export default class Widget {
            field1;
            arrow = () => 'arrow';
            greet() { return 'hello'; }
        }
        "#,
    )
    .expect("export default class should lower");

    let class = module
        .classes
        .iter()
        .find(|c| c.name == "Widget")
        .expect("class Widget should be in module.classes");
    assert!(class.is_exported, "class must be flagged exported");
    assert!(
        class.methods.iter().any(|m| m.name == "greet"),
        "prototype method greet should survive: {:?}",
        class.methods.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    let field_names: Vec<&str> = class.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(
        field_names.contains(&"field1") && field_names.contains(&"arrow"),
        "field initializers should survive: {field_names:?}"
    );
    assert!(
        has_named_export(&module, "Widget", "default"),
        "class must be registered as the default export: {:?}",
        module.exports
    );
}

#[test]
fn anonymous_export_default_class_lowers_full_body() {
    let module = lower_result(
        r#"
        export default class {
            greet() { return 'anon'; }
        }
        "#,
    )
    .expect("anonymous export default class should lower");

    let class = module
        .classes
        .iter()
        .find(|c| c.name == "default")
        .expect("anonymous default class should lower under synthetic name `default`");
    assert!(class.is_exported);
    assert!(
        class.methods.iter().any(|m| m.name == "greet"),
        "prototype method greet should survive"
    );
    assert!(
        has_named_export(&module, "default", "default"),
        "default export entry must exist: {:?}",
        module.exports
    );
}

#[test]
fn named_then_default_export_shape_still_works() {
    // The previously-working form (#665) — guard against regression.
    let module = lower_result(
        r#"
        class Widget2 { greet() { return 'hi2'; } }
        export default Widget2;
        "#,
    )
    .expect("named class + export default ident should lower");

    let class = module
        .classes
        .iter()
        .find(|c| c.name == "Widget2")
        .expect("class Widget2 should be in module.classes");
    assert!(class.is_exported);
    assert!(class.methods.iter().any(|m| m.name == "greet"));
    assert!(has_named_export(&module, "Widget2", "default"));
}
