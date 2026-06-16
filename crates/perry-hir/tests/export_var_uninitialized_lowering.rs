// Regression test for #5239: an uninitialized exported `var` (the canonical
// TypeScript-emitted enum/namespace IIFE pattern) must register an exported
// binding so consumers route through its value-getter instead of an undefined
// closure-wrapper symbol (`__perry_wrap_perry_fn_<src>__<Name>`).

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Export, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "export_var_uninit.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "export_var_uninit.ts")
                .expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn is_named_export(module: &Module, name: &str) -> bool {
    module.exports.iter().any(|export| {
        matches!(export, Export::Named { local, exported }
            if local == name && exported == name)
    })
}

#[test]
fn uninitialized_export_var_registers_exported_binding() {
    // The exact TypeScript enum IIFE shape from #5239.
    let module = lower_result(
        r#"
        export var Color;
        (function (Color) {
            Color["RED"] = "red";
            Color["BLUE"] = "blue";
        })(Color || (Color = {}));
        "#,
    );

    assert!(
        is_named_export(&module, "Color"),
        "uninitialized `export var Color;` must be a named export: {:?}",
        module.exports
    );
    assert!(
        module.exported_objects.contains(&"Color".to_string()),
        "uninitialized `export var Color;` must get an export global so the \
         value-getter is emitted (otherwise the consumer references an \
         undefined wrapper symbol): {:?}",
        module.exported_objects
    );

    // A backing `Stmt::Let` (declared `undefined`) must precede the IIFE
    // assignment in module init — without it codegen never emits the global
    // that backs the value-getter.
    let let_idx = module.init.iter().position(
        |s| matches!(s, Stmt::Let { name, init, .. } if name == "Color" && init.is_none()),
    );
    assert!(
        let_idx.is_some(),
        "expected a hoisted `Stmt::Let {{ name: \"Color\", init: None }}` in module init: {:?}",
        module.init
    );
}

#[test]
fn uninitialized_export_var_assigned_later_still_exports() {
    // General case (no IIFE): declare uninitialized, assign at module scope.
    let module = lower_result(
        r#"
        export var x;
        x = 41;
        x = x + 1;
        export let z;
        z = 5;
        "#,
    );

    for name in ["x", "z"] {
        assert!(
            is_named_export(&module, name),
            "`export var/let {name};` (assigned later) must be a named export: {:?}",
            module.exports
        );
        assert!(
            module.exported_objects.contains(&name.to_string()),
            "`export var/let {name};` must get an export global: {:?}",
            module.exported_objects
        );
    }
}

#[test]
fn initialized_export_var_still_exports() {
    // Regression: the initialized form must keep working (it hits the
    // `Some(init)` branch, not the new `else`).
    let module = lower_result("export var initialized = 1;");
    assert!(is_named_export(&module, "initialized"));
    assert!(module.exported_objects.contains(&"initialized".to_string()));
}
