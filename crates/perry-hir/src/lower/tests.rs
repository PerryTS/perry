//! Unit tests for `LoweringContext` registration and lookup helpers.
//!
//! Extracted from the inline `#[cfg(test)] mod tests { ... }` block at
//! the bottom of `lower/mod.rs` so the entry-point file stays under the
//! 2,000-LOC soft cap. Test bodies are unchanged — only the indentation
//! and the surrounding `mod tests` wrapper were stripped.

#![cfg(test)]

use super::*;
use crate::ir::EnumValue;
use perry_types::{Type, TypeParam};

fn make_ctx() -> LoweringContext {
    LoweringContext::new("test.ts")
}

#[test]
fn test_lower_define_and_lookup_local() {
    let mut ctx = make_ctx();
    let id = ctx.define_local("x".to_string(), Type::Number);
    assert_eq!(ctx.lookup_local("x"), Some(id));
    assert_eq!(ctx.lookup_local("y"), None);
    // Verify the type is stored correctly
    assert_eq!(ctx.lookup_local_type("x"), Some(&Type::Number));
}

#[test]
fn test_lower_function_registration() {
    let mut ctx = make_ctx();
    let func_id = ctx.fresh_func();
    ctx.register_func("myFunc".to_string(), func_id);

    assert_eq!(ctx.lookup_func("myFunc"), Some(func_id));
    assert_eq!(ctx.lookup_func("nonExistent"), None);
    // Reverse lookup by id
    assert_eq!(ctx.lookup_func_name(func_id), Some("myFunc"));
}

#[test]
fn test_lower_class_registration() {
    let mut ctx = make_ctx();
    let class_id = ctx.fresh_class();
    ctx.register_class("MyClass".to_string(), class_id);

    assert_eq!(ctx.lookup_class("MyClass"), Some(class_id));
    assert_eq!(ctx.lookup_class("Missing"), None);
}

#[test]
fn test_lower_local_shadowing() {
    let mut ctx = make_ctx();
    let id1 = ctx.define_local("x".to_string(), Type::Number);
    let id2 = ctx.define_local("x".to_string(), Type::String);

    // lookup_local uses .rev() so the latest definition wins
    assert_eq!(ctx.lookup_local("x"), Some(id2));
    assert_ne!(id1, id2);

    // The shadowed type should be String (the latest)
    assert_eq!(ctx.lookup_local_type("x"), Some(&Type::String));

    // Both entries still exist in the vec
    assert_eq!(ctx.locals.len(), 2);
}

#[test]
fn test_lower_function_shadowing() {
    let mut ctx = make_ctx();
    let id1 = ctx.fresh_func();
    let id2 = ctx.fresh_func();
    ctx.register_func("f".to_string(), id1);
    ctx.register_func("f".to_string(), id2);

    // lookup_func uses .rev() so the latest definition wins
    assert_eq!(ctx.lookup_func("f"), Some(id2));
}

#[test]
fn test_lower_imported_function_registration() {
    let mut ctx = make_ctx();
    ctx.register_imported_func("myRead".to_string(), "readFileSync".to_string());

    assert_eq!(ctx.lookup_imported_func("myRead"), Some("readFileSync"));
    assert_eq!(ctx.lookup_imported_func("unknown"), None);
}

#[test]
fn test_lower_builtin_module_alias() {
    let mut ctx = make_ctx();
    ctx.register_builtin_module_alias("myFs".to_string(), "fs".to_string());

    assert_eq!(ctx.lookup_builtin_module_alias("myFs"), Some("fs"));
    assert_eq!(ctx.lookup_builtin_module_alias("nope"), None);
}

#[test]
fn test_lower_enum_registration_and_member_lookup() {
    let mut ctx = make_ctx();
    let enum_id = ctx.fresh_enum();
    ctx.define_enum(
        "Color".to_string(),
        enum_id,
        vec![
            ("Red".to_string(), EnumValue::Number(0)),
            ("Green".to_string(), EnumValue::Number(1)),
            ("Blue".to_string(), EnumValue::Number(2)),
        ],
    );

    let (looked_up_id, members) = ctx.lookup_enum("Color").unwrap();
    assert_eq!(looked_up_id, enum_id);
    assert_eq!(members.len(), 3);

    assert!(matches!(
        ctx.lookup_enum_member("Color", "Red"),
        Some(EnumValue::Number(0))
    ));
    assert!(ctx.lookup_enum_member("Color", "Yellow").is_none());
    assert!(ctx.lookup_enum("Missing").is_none());
}

#[test]
fn test_lower_class_statics() {
    let mut ctx = make_ctx();
    ctx.register_class_statics(
        "MyClass".to_string(),
        vec!["count".to_string()],
        vec!["create".to_string()],
    );

    assert!(ctx.has_static_field("MyClass", "count"));
    assert!(!ctx.has_static_field("MyClass", "missing"));
    assert!(ctx.has_static_method("MyClass", "create"));
    assert!(!ctx.has_static_method("MyClass", "missing"));
    assert!(!ctx.has_static_field("Other", "count"));
}

#[test]
fn test_lower_native_module_registration() {
    let mut ctx = make_ctx();
    // Namespace import: import * as fs from "fs"
    ctx.register_native_module("fs".to_string(), "fs".to_string(), None);
    // Named import: import { v4 as uuid } from "uuid"
    ctx.register_native_module(
        "uuid".to_string(),
        "uuid".to_string(),
        Some("v4".to_string()),
    );

    let (module, method) = ctx.lookup_native_module("fs").unwrap();
    assert_eq!(module, "fs");
    assert_eq!(method, None);

    let (module, method) = ctx.lookup_native_module("uuid").unwrap();
    assert_eq!(module, "uuid");
    assert_eq!(method, Some("v4"));

    assert!(ctx.lookup_native_module("missing").is_none());
}

#[test]
fn test_native_module_binding_value_named_import() {
    // #5242: a named builtin import (`import { relative } from 'path'`) used
    // as a value (e.g. an object-literal shorthand `{ relative }`) must resolve
    // to the callable builtin — `path.relative` — not be dropped to undefined.
    let mut ctx = make_ctx();
    ctx.register_native_module(
        "relative".to_string(),
        "path".to_string(),
        Some("relative".to_string()),
    );
    let value = super::lower_expr::native_module_binding_value(&ctx, "relative");
    match value {
        crate::ir::Expr::PropertyGet {
            object, property, ..
        } => {
            assert_eq!(property, "relative");
            assert!(matches!(*object, crate::ir::Expr::NativeModuleRef(ref m) if m == "path"));
        }
        other => panic!("expected PropertyGet(path.relative), got {other:?}"),
    }
}

#[test]
fn test_native_module_binding_value_os_eol() {
    // `import { EOL } from 'os'` resolves to the OsEOL intrinsic value, whether
    // used directly or as a shorthand property.
    let mut ctx = make_ctx();
    ctx.register_native_module("EOL".to_string(), "os".to_string(), Some("EOL".to_string()));
    let value = super::lower_expr::native_module_binding_value(&ctx, "EOL");
    assert!(matches!(value, crate::ir::Expr::OsEOL));
}

#[test]
fn test_native_module_binding_value_namespace() {
    // A namespace import of a non-CJS-style native module (method_name None)
    // resolves to a bare NativeModuleRef — the value used as a shorthand
    // property must match what the bare identifier reference produces.
    let mut ctx = make_ctx();
    ctx.register_native_module("crypto".to_string(), "crypto".to_string(), None);
    let value = super::lower_expr::native_module_binding_value(&ctx, "crypto");
    assert!(matches!(value, crate::ir::Expr::NativeModuleRef(ref m) if m == "crypto"));
}

#[test]
fn test_lower_type_param_scoping() {
    let mut ctx = make_ctx();
    assert!(!ctx.is_type_param("T"));

    ctx.enter_type_param_scope(&[TypeParam {
        name: "T".to_string(),
        constraint: None,
        default: None,
    }]);
    assert!(ctx.is_type_param("T"));
    assert!(!ctx.is_type_param("U"));

    // Nested scope
    ctx.enter_type_param_scope(&[TypeParam {
        name: "U".to_string(),
        constraint: None,
        default: None,
    }]);
    assert!(ctx.is_type_param("T")); // outer scope still visible
    assert!(ctx.is_type_param("U"));

    ctx.exit_type_param_scope();
    assert!(ctx.is_type_param("T"));
    assert!(!ctx.is_type_param("U")); // inner scope gone

    ctx.exit_type_param_scope();
    assert!(!ctx.is_type_param("T")); // all scopes gone
}

#[test]
fn test_lower_fresh_ids_increment() {
    let mut ctx = make_ctx();
    assert_eq!(ctx.fresh_local(), 0);
    assert_eq!(ctx.fresh_local(), 1);
    assert_eq!(ctx.fresh_local(), 2);

    assert_eq!(ctx.fresh_func(), 0);
    assert_eq!(ctx.fresh_func(), 1);

    // Classes start at 1 (default for new())
    assert_eq!(ctx.fresh_class(), 1);
    assert_eq!(ctx.fresh_class(), 2);
}

#[test]
fn test_lower_namespace_var_lookup() {
    let mut ctx = make_ctx();
    let local_id = ctx.define_local("Utils_helper".to_string(), Type::Number);
    ctx.namespace_vars
        .push(("Utils".to_string(), "helper".to_string(), local_id));

    assert_eq!(ctx.lookup_namespace_var("Utils", "helper"), Some(local_id));
    assert_eq!(ctx.lookup_namespace_var("Utils", "missing"), None);
    assert_eq!(ctx.lookup_namespace_var("Other", "helper"), None);
}

/// Run `f` on a thread with the same large (128 MB) stack the real compiler
/// uses for its collect/lower walk (`perry-main`, see `crates/perry/src/
/// main.rs`). The default cargo-test harness thread is only ~2 MB, which is
/// far too small to parse or lower the multi-thousand-node chains these
/// `#5259` tests build — without this, parsing/lowering them would overflow
/// the *test* stack before the depth guard ever fires.
fn run_with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new()
        .stack_size(128 * 1024 * 1024)
        .spawn(f)
        .expect("spawn large-stack thread")
        .join()
        .expect("test body panicked");
}

/// #5259: deeply-nested expression chains must surface a diagnostic instead
/// of overflowing the native stack and SIGABRT-ing the whole process. Each
/// shape (binary `1+1+...`, member `o.a.a....`, logical `a||a||...`) recurses
/// once per node in `lower_expr`; past `MAX_EXPR_CHAIN_LOWER_DEPTH` lowering
/// bails with a "nested too deeply" error rather than recursing further.
fn assert_too_deep(source: String) {
    run_with_large_stack(move || {
        let module =
            perry_parser::parse_typescript(&source, "deep.ts").expect("source should parse fine");
        let err = super::lower_module(&module, "deep", "deep.ts")
            .expect_err("deeply-nested expression must be rejected, not lowered");
        let msg = format!("{err}");
        assert!(
            msg.contains("nested too deeply"),
            "expected a depth diagnostic, got: {msg}"
        );
    });
}

#[test]
fn test_lower_rejects_deep_binary_chain() {
    let n = (super::lower_expr::MAX_EXPR_CHAIN_LOWER_DEPTH as usize) + 2;
    let chain: Vec<&str> = vec!["1"; n];
    assert_too_deep(format!("var x = {};\n", chain.join("+")));
}

#[test]
fn test_lower_rejects_deep_member_chain() {
    let n = (super::lower_expr::MAX_EXPR_CHAIN_LOWER_DEPTH as usize) + 1;
    assert_too_deep(format!("var o = {{}};\nvar x = o{};\n", ".a".repeat(n)));
}

#[test]
fn test_lower_rejects_deep_logical_chain() {
    let n = (super::lower_expr::MAX_EXPR_CHAIN_LOWER_DEPTH as usize) + 2;
    let chain: Vec<&str> = vec!["a"; n];
    assert_too_deep(format!("var a = 0;\nvar x = {};\n", chain.join("||")));
}

/// #5271: the perf index over `native_instances` must reproduce the old
/// reverse-scan semantics exactly — innermost (last-registered) binding wins,
/// and `truncate_native_instances` re-exposes the outer binding when the inner
/// scope pops. Mirrors the `lookup_native_instance` last-match-wins rule.
#[test]
fn test_native_instance_index_shadowing_and_truncation() {
    let mut ctx = make_ctx();
    // Outer binding `e` -> events/EventEmitter.
    ctx.register_native_instance(
        "e".to_string(),
        "events".to_string(),
        "EventEmitter".to_string(),
    );
    assert_eq!(
        ctx.lookup_native_instance("e"),
        Some(("events", "EventEmitter"))
    );

    // Enter an inner scope: shadow `e` with a different native type.
    let mark = ctx.native_instances.len();
    ctx.register_native_instance(
        "e".to_string(),
        "stream".to_string(),
        "Readable".to_string(),
    );
    // Inner (last) binding wins.
    assert_eq!(
        ctx.lookup_native_instance("e"),
        Some(("stream", "Readable"))
    );

    // Pop the inner scope: the outer binding must be restored.
    ctx.truncate_native_instances(mark);
    assert_eq!(
        ctx.lookup_native_instance("e"),
        Some(("events", "EventEmitter"))
    );

    // Pop the outer binding too: no entry remains.
    ctx.truncate_native_instances(0);
    assert!(ctx.lookup_native_instance("e").is_none());
}

/// #5271: module-level native instances (never truncated) keep last-match-wins
/// via the overwrite index, matching the old reverse scan of the fallback arm.
#[test]
fn test_module_native_instance_index_last_wins() {
    let mut ctx = make_ctx();
    ctx.push_module_native_instance((
        "db".to_string(),
        "mongodb".to_string(),
        "MongoClient".to_string(),
    ));
    assert_eq!(
        ctx.lookup_native_instance("db"),
        Some(("mongodb", "MongoClient"))
    );
    // A later registration of the same name shadows the earlier one.
    ctx.push_module_native_instance((
        "db".to_string(),
        "mysql2/promise".to_string(),
        "Pool".to_string(),
    ));
    assert_eq!(
        ctx.lookup_native_instance("db"),
        Some(("mysql2/promise", "Pool"))
    );
}

/// #5271 perf gate (run with `--release --ignored`): time M lookups against a
/// K-sized registry to show indexed lookups are ~flat in K (O(1)) rather than
/// O(K) per call. Prints timings; not asserted (machine-dependent) but the
/// flatness across K is the observable signal. Covers the registries whose
/// linear scans this change indexed.
#[test]
#[ignore]
fn perf_registry_lookup_is_flat_in_k() {
    use std::time::Instant;
    const M: usize = 20_000;
    for k in [0usize, 2_000, 8_000, 16_000] {
        let mut ctx = make_ctx();
        for i in 0..k {
            ctx.register_class_statics(
                format!("K{i}"),
                vec![format!("f{i}")],
                vec![format!("s{i}")],
            );
            ctx.register_native_instance(format!("ni{i}"), "events".into(), "EventEmitter".into());
            ctx.register_native_module(format!("nm{i}"), "fs".into(), None);
        }
        // The hot case the bug targets: the receiver is NOT in the registry, so
        // the old reverse/forward scan walked the whole Vec and returned None.
        let t = Instant::now();
        let mut acc = 0u64;
        for _ in 0..M {
            acc += ctx.has_static_method("Missing", "s") as u64;
            acc += ctx.lookup_native_instance("missing").is_some() as u64;
            acc += ctx.lookup_native_module("missing").is_some() as u64;
        }
        eprintln!("K={k:<6} {M} x3 lookups: {:?}  (acc={acc})", t.elapsed());
    }
}

/// A chain comfortably under the ceiling still lowers cleanly — the guard
/// must not reject ordinary (if large) expressions.
#[test]
fn test_lower_accepts_chain_under_limit() {
    run_with_large_stack(|| {
        let n = (super::lower_expr::MAX_EXPR_CHAIN_LOWER_DEPTH as usize) / 2;
        let chain: Vec<&str> = vec!["1"; n];
        let source = format!("var x = {};\n", chain.join("+"));
        let module = perry_parser::parse_typescript(&source, "ok.ts").expect("parses");
        assert!(
            super::lower_module(&module, "ok", "ok.ts").is_ok(),
            "a chain under the depth ceiling must lower without error"
        );
    });
}

/// A `class A extends Base` whose parent Ident is an in-scope LEXICAL LOCAL
/// (a `let`/`const`/param), not a class, must be lowered with NO static
/// `extends_name` — the parent is resolved purely dynamically via
/// `extends_expr`. Retaining a static `extends_name` lets the codegen
/// parent-chain walks (packed-keys field layout, `js_register_class_parent`
/// edge, inherited-method / vtable install, type-facts) re-resolve the bare
/// name through the module-wide name→class map to an UNRELATED same-named class
/// — e.g. a function-local `class Base` that leaked into that map — corrupting
/// the subclass's field layout and inheritance. (Regression: a large minified
/// program's zod `let Y=_?.Parent??Object; class A extends Y{}` wrongly
/// inherited a captured iterator class `Y`'s private `#q`, throwing "Cannot
/// access private member from an object whose class did not declare it".)
#[test]
fn test_lexically_shadowed_heritage_drops_static_extends_name() {
    let source = r#"
        function make(spec) {
            let Base = (spec && spec.Parent) || Object;
            class A extends Base {}
            return A;
        }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let a = hir
        .classes
        .iter()
        .find(|c| c.name == "A")
        .expect("class A is lowered");
    assert!(
        a.heritage_lexically_shadowed,
        "`Base` is a lexical local, so `class A extends Base` is lexically shadowed"
    );
    assert_eq!(
        a.extends_name, None,
        "a lexically-shadowed heritage must NOT retain a static extends_name — \
         it would re-resolve to an unrelated same-named class"
    );
    assert_eq!(
        a.extends, None,
        "no static parent class id for a dynamically-resolved parent"
    );
    assert!(
        a.extends_expr.is_some(),
        "the parent is resolved dynamically via extends_expr"
    );
}

/// A normal subclass whose parent is a CLASS DECLARATION (not a local) is
/// unaffected by the shadowed-heritage handling: class declarations are not in
/// `ctx.locals`, so the heritage is NOT lexically shadowed and static parent
/// resolution (field/method inheritance) is preserved.
#[test]
fn test_plain_class_to_class_heritage_keeps_static_extends_name() {
    let source = r#"
        class Base { x = 1; }
        class Sub extends Base { y = 2; }
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");
    let sub = hir
        .classes
        .iter()
        .find(|c| c.name == "Sub")
        .expect("class Sub is lowered");
    assert!(
        !sub.heritage_lexically_shadowed,
        "a class-declaration parent is not a lexical local"
    );
    assert_eq!(
        sub.extends_name.as_deref(),
        Some("Base"),
        "static class-to-class heritage keeps its extends_name"
    );
}

/// #6679: a NAMED class EXPRESSION's `.name` is its own explicit name
/// (`Named` in `const B = class Named {}`), not the outer binding name. Per
/// spec a named class expression is not an anonymous function definition, so
/// the assignment's NamedEvaluation (`SetFunctionName` from `const B =`) must
/// not clobber the declared name. The module-top-level `const X = class {…}`
/// fast path registers the class under the binding name so `new B()` /
/// `instanceof B` resolve statically, and records a `class_display_names`
/// override to the explicit name for codegen to emit as `.name`. An ANONYMOUS
/// `const A = class {}` takes the inferred binding name and needs no override.
#[test]
fn test_named_class_expression_var_decl_reports_explicit_name() {
    let source = r#"
        const B = class Named {};
        const A = class {};
    "#;
    let module = perry_parser::parse_typescript(source, "t.ts").expect("source parses");
    let hir = super::lower_module(&module, "t", "t.ts").expect("source lowers");

    let named = hir
        .classes
        .iter()
        .find(|c| c.name == "B")
        .expect("class registered under binding name `B`");
    assert_eq!(
        hir.class_display_names.get(&named.id).map(String::as_str),
        Some("Named"),
        "named class expression must report its explicit name as `.name`"
    );

    let anon = hir
        .classes
        .iter()
        .find(|c| c.name == "A")
        .expect("anonymous class registered under inferred name `A`");
    assert_eq!(
        hir.class_display_names.get(&anon.id),
        None,
        "anonymous class expression uses the inferred binding name, no override"
    );
}
