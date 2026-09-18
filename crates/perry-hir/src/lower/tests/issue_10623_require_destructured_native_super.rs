//! #10623: `class NoCtor extends AsyncResource {}` — a constructor-less
//! subclass of a native base obtained via `const { AsyncResource } =
//! require("node:async_hooks")` — must still resolve `AsyncResource` as the
//! NATIVE parent (`native_extends`), not as a dynamically-shadowed local
//! (`extends_expr`). Split from `tests.rs` for the 2000-line cap.
//!
//! A CJS-wrapped module runs its whole body inside the wrap's synthetic
//! `require(...)` IIFE (see `test_cjs_wrapper_lru_cache_destructure_uses_
//! static_constructor` above for the same simulated-wrapper shape), so every
//! top-level `const` there — including `const { AsyncResource } =
//! require(...)` — is a genuine local. Before the fix, `class_decl.rs`'s
//! `locally_shadowed` check could not tell that apart from a real user
//! shadow (`const AsyncResource = MyOwnClass`), so it always took the dynamic
//! `extends_expr` path and lost the native install + argument forwarding.

fn cjs_wrapper_source(body: &str) -> String {
    format!(
        r#"
        function __perry_cjs_require_error(kind: string, code: string, message: string): any {{
            return {{ kind, code, message }};
        }}
        function __perry_cjs_require_is_builtin(specifier: string): boolean {{
            return false;
        }}
        function require(specifier: string): any {{
            return undefined;
        }}
        {body}
        "#
    )
}

/// The issue's exact shape: no own constructor. Must resolve as the native
/// parent, forwarding the `new`-site args to `super()` implicitly.
#[test]
fn cjs_destructured_async_resource_implicit_ctor_uses_native_parent() {
    let source = cjs_wrapper_source(
        r#"
        const { AsyncResource } = require("node:async_hooks");
        class NoCtor extends AsyncResource {}
        const a = new NoCtor("MyResource");
        "#,
    );
    let module = perry_parser::parse_typescript(&source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let class = hir
        .classes
        .iter()
        .find(|c| c.name == "NoCtor")
        .expect("NoCtor is lowered");
    assert_eq!(
        class.native_extends,
        Some(("async_hooks".to_string(), "AsyncResource".to_string())),
        "a require()-destructured AsyncResource must resolve as the native \
         parent, not a dynamically-shadowed local: {class:#?}"
    );
    assert!(
        class.extends_expr.is_none(),
        "the native parent must not ALSO be captured as a dynamic \
         extends_expr (that is the pre-fix shadowed-local path): {class:#?}"
    );
}

/// The explicit-`super()` control: this form must keep resolving natively
/// too — before the fix it took the SAME broken dynamic path (the issue's
/// claim that the explicit form "already works" held only for an ESM import,
/// not for this CJS shape).
#[test]
fn cjs_destructured_async_resource_explicit_ctor_uses_native_parent() {
    let source = cjs_wrapper_source(
        r#"
        const { AsyncResource } = require("node:async_hooks");
        class WithCtor extends AsyncResource {
            constructor(type: string) { super(type); }
        }
        const b = new WithCtor("MyResource2");
        "#,
    );
    let module = perry_parser::parse_typescript(&source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let class = hir
        .classes
        .iter()
        .find(|c| c.name == "WithCtor")
        .expect("WithCtor is lowered");
    assert_eq!(
        class.native_extends,
        Some(("async_hooks".to_string(), "AsyncResource".to_string())),
        "the explicit-ctor form must ALSO resolve as the native parent: {class:#?}"
    );
}

/// A class EXPRESSION reaches a separate lowering arm
/// (`lower_class_from_ast`) with its own copy of the shadow check — pin it
/// too so the fix is not name-keyed to only the declaration form.
#[test]
fn cjs_destructured_async_resource_class_expr_uses_native_parent() {
    let source = cjs_wrapper_source(
        r#"
        const { AsyncResource } = require("node:async_hooks");
        const Anon = class extends AsyncResource {};
        const inst = new Anon("AnonResource");
        "#,
    );
    let module = perry_parser::parse_typescript(&source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let class = hir
        .classes
        .iter()
        .find(|c| c.name == "Anon")
        .expect("the class expression is lowered");
    assert_eq!(
        class.native_extends,
        Some(("async_hooks".to_string(), "AsyncResource".to_string())),
        "a class EXPRESSION extending a require()-destructured native base \
         must ALSO resolve natively: {class:#?}"
    );
}

/// Guards the other side: GENUINE shadowing (the user's own value, not a
/// require() re-export) must still take the dynamic `extends_expr` path —
/// the fix narrows the false positive, it does not remove the real check.
#[test]
fn cjs_local_shadowing_a_native_name_still_goes_dynamic() {
    let source = cjs_wrapper_source(
        r#"
        class MyOwnAsyncResource { tag = "mine"; }
        const AsyncResource = MyOwnAsyncResource;
        class NoCtor extends AsyncResource {}
        const a = new NoCtor();
        "#,
    );
    let module = perry_parser::parse_typescript(&source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let class = hir
        .classes
        .iter()
        .find(|c| c.name == "NoCtor")
        .expect("NoCtor is lowered");
    assert!(
        class.native_extends.is_none(),
        "a genuine user shadow of the native name must NOT resolve natively: {class:#?}"
    );
    assert!(
        class.extends_expr.is_some(),
        "a genuine user shadow must still route through the dynamic parent: {class:#?}"
    );
}
