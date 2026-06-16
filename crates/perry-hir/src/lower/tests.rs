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
        crate::ir::Expr::PropertyGet { object, property } => {
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
