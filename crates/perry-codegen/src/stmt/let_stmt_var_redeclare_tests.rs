//! #10488: a hoisted `var` reaches `lower_let` as TWO `Stmt::Let`s sharing one
//! local id — a body-entry predefine (`Any = undefined`), then the real
//! declaration (here, `Array(Number) = [0]`) — and the second one takes the
//! #1803 redeclaration early return. `ctx.local_types` must be refreshed on
//! that path too, or `expr_may_return_boxed_value_from_raw_f64_fallback`
//! (which reads it via `static_type_of`) stays desynced from
//! `is_numeric_expr` (which reads the separately-refreshed
//! `proven_local_types`), and a strict-equality compare against an
//! out-of-bounds array read wrongly takes the bare-`fcmp` numeric fast path.

use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Stmt};

use crate::temp_root_coverage::main_ir_for as ir_for;

const ARR: u32 = 1;
const R: u32 = 2;

/// A CALL to the boxed comparison helper, not the unconditional `declare`
/// line — mirrors `compare_tests.rs`'s `JS_EQ_CALL`.
const JS_EQ_CALL: &str = "call i64 @js_eq(";

/// Hand-build the exact HIR shape a hoisted `var arr = [0]; ...; arr[1] ===
/// void 0;` lowers to: TWO `Let`s sharing id `ARR` (the predefine, then the
/// real array-literal declaration), followed by a strict-equality compare of
/// an out-of-bounds index read against `Expr::Void`.
#[test]
fn var_redeclared_numeric_array_compare_against_void_stays_boxed() {
    let ir = ir_for(
        "var_redeclare_void_compare",
        vec![
            // Body-entry predefine: `var arr;` before the real declaration
            // runs — declared `Any`, matching what hoisting emits.
            Stmt::Let {
                id: ARR,
                name: "arr".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            // The real declaration: same id, now a proven `Array(Number)`.
            // This is the REDECLARATION path in `lower_let` (#1803).
            Stmt::Let {
                id: ARR,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::Array(vec![Expr::Integer(0)])),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(ARR)),
                        index: Box::new(Expr::Integer(1)),
                    }),
                    right: Box::new(Expr::Void(Box::new(Expr::Integer(0)))),
                }),
            },
        ],
    );
    assert!(
        ir.contains(JS_EQ_CALL),
        "an out-of-bounds read of a var-redeclared Array(Number) compared \
         against `void 0` must fall back to the boxed js_eq helper (the \
         element read can be undefined, which the STATIC numeric fast path \
         can't represent); got IR:\n{ir}"
    );
    // NOT a blanket "no fcmp anywhere" check: `js_eq`'s own dynamic
    // comparison lowering has an internal, RUNTIME-guarded fast path (an
    // `icmp` range check on the raw bits proves both operands are genuine
    // untagged doubles before it dares an `fcmp`) that is safe and expected
    // to appear here too — that is a property of the boxed path, not the
    // STATIC always-numeric bug this test guards against. The call to
    // `js_eq` above is what proves this comparison did NOT take the static
    // fast path (`lower_strict_eq_against_number`, which emits an
    // UNGUARDED `fcmp` with no dynamic dispatch at all).
}

/// Control: the SAME shape through a `let` (no redeclaration ambiguity)
/// already took the boxed path before this fix and must keep doing so.
#[test]
fn let_numeric_array_compare_against_void_stays_boxed() {
    let ir = ir_for(
        "let_void_compare",
        vec![
            Stmt::Let {
                id: ARR,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::Array(vec![Expr::Integer(0)])),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(ARR)),
                        index: Box::new(Expr::Integer(1)),
                    }),
                    right: Box::new(Expr::Void(Box::new(Expr::Integer(0)))),
                }),
            },
        ],
    );
    assert!(
        ir.contains(JS_EQ_CALL),
        "control case (let, no redeclaration) must already take the boxed \
         path; got IR:\n{ir}"
    );
}
