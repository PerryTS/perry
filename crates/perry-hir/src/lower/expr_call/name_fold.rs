//! `<BuiltinFn>.name` / `Object.getOwnPropertyDescriptor(<BuiltinFn>, "name")`
//! HIR-level folds (#2144).
//!
//! Built-in constructors (`TypeError`, `Promise`, …) and the static functions
//! on built-in namespaces (`Math.min`, `Promise.race`, `Array.isArray`, …)
//! are not represented as named closure values in Perry — bare reads of them
//! return the 0 sentinel. The direct `.name` access is folded in
//! `lower/expr_member.rs`; the descriptor-form access (the spec defines
//! `name` as `{ value, writable:false, enumerable:false, configurable:true }`)
//! is folded by callers in `expr_call/native_module.rs`. Both rely on the
//! AST-shape recognizer here.

use swc_ecma_ast as ast;

use crate::analysis::{
    builtin_global_function_length, builtin_static_function_length, is_builtin_global_value_name,
    is_builtin_static_function_member,
};
use crate::ir::Expr;

/// If `arg` is an AST shape we recognize as a built-in function value —
/// either `Ident<BuiltinCtor>` (`TypeError`, `Promise`, …) or
/// `Member(Ident<Builtin>, Ident<staticFn>)` (`Math.min`, `Promise.race`,
/// …) — return the spec `name` string for that function. Otherwise None.
///
/// Local shadowing is NOT checked here (the bare-AST recognizer can't see
/// the scope), so call sites must verify the lowered receiver looks like
/// the global intrinsic — same gating logic as the direct-`.name` fold.
/// In practice both call sites operate on chains where the receiver
/// already lowered to a `GlobalGet(0)`/`PropertyGet { GlobalGet(0), … }`
/// shape, so the AST-only recognizer is a safe complement.
pub(super) fn builtin_fn_name_for_arg(arg: &ast::Expr) -> Option<String> {
    match arg {
        ast::Expr::Ident(id) => {
            let name = id.sym.as_ref();
            if is_builtin_global_value_name(name) {
                Some(name.to_string())
            } else {
                None
            }
        }
        ast::Expr::Member(m) => {
            if let (ast::Expr::Ident(ns_ident), ast::MemberProp::Ident(method_ident)) =
                (m.obj.as_ref(), &m.prop)
            {
                let ns = ns_ident.sym.as_ref();
                let method = method_ident.sym.as_ref();
                if is_builtin_static_function_member(ns, method) {
                    return Some(method.to_string());
                }
            }
            None
        }
        _ => None,
    }
}

/// If `arg` is an AST shape we recognize as a built-in static function value,
/// return its spec `.length`.
pub(super) fn builtin_fn_length_for_arg(arg: &ast::Expr) -> Option<u32> {
    match arg {
        ast::Expr::Ident(id) => builtin_global_function_length(id.sym.as_ref()),
        ast::Expr::Member(m) => {
            if let (ast::Expr::Ident(ns_ident), ast::MemberProp::Ident(method_ident)) =
                (m.obj.as_ref(), &m.prop)
            {
                return builtin_static_function_length(
                    ns_ident.sym.as_ref(),
                    method_ident.sym.as_ref(),
                );
            }
            None
        }
        _ => None,
    }
}

/// Build the spec-compliant data descriptor for a function `.name` or `.length`
/// property — `{ value, writable:false, enumerable:false, configurable:true }`.
/// Mirrors `js_object_get_own_property_descriptor` in the runtime; the two
/// must stay in sync.
pub(super) fn builtin_data_descriptor(value: Expr) -> Expr {
    Expr::Object(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Expr::Bool(false)),
        ("enumerable".to_string(), Expr::Bool(false)),
        ("configurable".to_string(), Expr::Bool(true)),
    ])
}

pub(super) fn name_data_descriptor(fname: String) -> Expr {
    builtin_data_descriptor(Expr::String(fname))
}
