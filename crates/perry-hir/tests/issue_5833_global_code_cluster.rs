//! Issue #5833 — test262 `language/global-code` / `language/identifier-resolution`
//! cluster. Covers two independent lowering fixes landed together because
//! both surfaced from the same script-top-level GlobalDeclarationInstantiation
//! worklist:
//!
//! - A top-level `class` binding that's reassigned somewhere in the module
//!   must actually take the new value on read (GlobalDeclarationInstantiation:
//!   class declarations are mutable bindings, unlike `const`).
//! - A top-level `let`/`const`/`class` name colliding with a restricted global
//!   property (`undefined`/`NaN`/`Infinity`) is an early SyntaxError for the
//!   entry module compiled as a Script.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, lower_module_with_class_id_types_seed_and_entry, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "issue_5833.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "issue_5833.ts")
}

fn lower_entry_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "issue_5833_entry.ts", &mut cache)?;
    lower_module_with_class_id_types_seed_and_entry(
        &parsed.module,
        "test",
        "issue_5833_entry.ts",
        1,
        None,
        None,
        None,
        true,
    )
    .map(|(module, _)| module)
}

fn class_local_let<'a>(module: &'a perry_hir::Module, name: &str) -> Option<&'a Expr> {
    module.init.iter().find_map(|stmt| match stmt {
        Stmt::Let {
            name: n,
            init: Some(init),
            mutable,
            ..
        } if n == name => {
            assert!(*mutable, "class binding local must be mutable");
            Some(init)
        }
        _ => None,
    })
}

#[test]
fn reassigned_top_level_class_gets_a_mutable_local_binding() {
    // test262 language/global-code/decl-lex.js: `class Foo {}; Foo = 5;`
    // must actually rebind `Foo` — previously this was a silent no-op (the
    // class name only ever resolved through the immutable class registry).
    let module = lower_src(
        r#"
        class Foo {}
        Foo = 5;
        console.log(Foo);
        "#,
    )
    .expect("reassigned class declaration should lower");

    let init = class_local_let(&module, "Foo")
        .expect("a reassigned class name must get a real local binding");
    assert!(
        matches!(init, Expr::ClassRef(_)),
        "the local's initial value should still be the class ref: {init:?}"
    );
}

#[test]
fn never_reassigned_top_level_class_keeps_pure_classref_resolution() {
    // Guard against the naive "always seed a local" fix: a class that's
    // never reassigned must NOT get a local binding, or `export default
    // Widget;`-style call sites that pattern-match `Expr::ClassRef` after
    // lowering a bare class-name identifier would silently break (#665).
    let module = lower_src(
        r#"
        class Widget {}
        console.log(Widget);
        "#,
    )
    .expect("plain class declaration should lower");

    assert!(
        class_local_let(&module, "Widget").is_none(),
        "a never-reassigned class must not get a local binding"
    );
}

#[test]
fn entry_script_rejects_lexical_undefined_binding() {
    // test262 language/global-code/decl-lex-restricted-global.js: `let
    // undefined;` collides with the non-configurable `undefined` value
    // property of a pristine global object — an early SyntaxError.
    let err = lower_entry_src("let undefined;")
        .expect_err("`let undefined` at entry-script top level must be rejected");
    assert!(
        err.to_string().contains("SyntaxError"),
        "expected a SyntaxError, got: {err}"
    );
}

#[test]
fn entry_script_rejects_nan_and_infinity_lexical_bindings_too() {
    for name in ["NaN", "Infinity"] {
        let src = format!("const {name} = 1;");
        let err = lower_entry_src(&src).expect_err("this const declaration must be rejected");
        assert!(
            err.to_string().contains("SyntaxError"),
            "expected a SyntaxError for `{name}`, got: {err}"
        );
    }
}

#[test]
fn restricted_global_check_does_not_reject_ordinary_top_level_names() {
    lower_entry_src("let ordinaryName = 1; const another = 2; class Whatever {}")
        .expect("ordinary top-level lexical names must not be rejected");
}

#[test]
fn restricted_global_check_only_applies_to_the_entry_script() {
    // A module compiled without `is_entry_module` (an imported module, or any
    // non-entry lowering caller) never reaches GlobalDeclarationInstantiation
    // against the real global object, so the same source must NOT be rejected.
    lower_src("let undefined;").expect(
        "a non-entry module's `let undefined` must not be rejected \
         (it never binds against the real global object)",
    );
}
