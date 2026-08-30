//! IR coverage for the shared numeric guard on fully dynamic `+` trees.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Stmt};

use crate::temp_root_coverage::main_ir_for as ir_for;

const A: u32 = 1;
const B: u32 = 2;
const C: u32 = 3;
const RESULT: u32 = 4;

fn any_local(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn erased_bigint_local(id: u32, name: &str, value: &str) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        },
        Stmt::Expr(Expr::LocalSet(
            id,
            Box::new(Expr::BigInt(value.to_string())),
        )),
    ]
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn arithmetic(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn dynamic_locals() -> Vec<Stmt> {
    vec![
        any_local(A, "a", Expr::Undefined),
        any_local(B, "b", Expr::Undefined),
        any_local(C, "c", Expr::Undefined),
    ]
}

fn result(expr: Expr) -> Stmt {
    Stmt::Let {
        id: RESULT,
        name: "result".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(expr),
    }
}

#[test]
fn three_leaf_dynamic_add_tree_uses_one_shared_guard() {
    let mut body = dynamic_locals();
    body.push(result(add(
        Expr::LocalGet(A),
        add(Expr::LocalGet(B), Expr::LocalGet(C)),
    )));
    let ir = ir_for("three_leaf_dynamic_add_tree", body);

    assert_eq!(
        ir.matches("\nguarded_add.numeric.").count(),
        1,
        "the tree should have one shared numeric block:\n{ir}"
    );
    assert_eq!(
        ir.matches("fadd double").count(),
        2,
        "the fast arm must preserve both additions:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_dynamic_string_or_number_add(")
            .count(),
        2,
        "the cold arm must preserve both dynamic additions:\n{ir}"
    );
}

#[test]
fn two_leaf_dynamic_add_takes_the_guard_too() {
    // This test previously asserted the opposite, on the cost model that "a
    // single dynamic add should not pay for a separate guard diamond" — the
    // guard was thought merely to move the helper behind a branch. It does
    // more: the hot arm becomes an inline `fadd`, and the operands stop going
    // through `lower_rooted_dynamic_binary`, which roots them. Measured on
    // `s += v` with both operands numbers at runtime but neither statically
    // proven, the pair guard is worth 3.44 -> 1.33 ns/op (#9157).
    let mut body = dynamic_locals();
    body.push(result(add(Expr::LocalGet(A), Expr::LocalGet(B))));
    let ir = ir_for("two_leaf_dynamic_add", body);

    assert_eq!(
        ir.matches("\nguarded_add.numeric.").count(),
        1,
        "a two-leaf tree should get one guard diamond:\n{ir}"
    );
    assert_eq!(
        ir.matches("fadd double").count(),
        1,
        "the fast arm must perform the addition inline:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_dynamic_string_or_number_add(")
            .count(),
        1,
        "the cold arm must preserve exact dynamic `+` semantics:\n{ir}"
    );
}

#[test]
fn dynamic_arithmetic_results_are_guarded_before_add() {
    // #9143: for-of element bindings can be `Any` even when their runtime
    // values are BigInts. The nested arithmetic helpers preserve BigInt, so
    // their boxed results must not feed an unconditional native `fadd`.
    let mut body = erased_bigint_local(A, "a", "123456789012345678901234567890");
    body.extend(erased_bigint_local(B, "d", "1000000007"));
    let quotient = arithmetic(BinaryOp::Div, Expr::LocalGet(A), Expr::LocalGet(B));
    let product = arithmetic(BinaryOp::Mul, quotient, Expr::LocalGet(B));
    let remainder = arithmetic(BinaryOp::Mod, Expr::LocalGet(A), Expr::LocalGet(B));
    body.push(result(add(product, remainder)));
    let ir = ir_for("dynamic_bigint_identity_add", body);

    assert!(
        ir.contains("\nguarded_add.numeric."),
        "possibly-BigInt arithmetic results need a runtime number guard:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add("),
        "the non-number arm must preserve BigInt addition:\n{ir}"
    );
    for helper in ["js_dynamic_div", "js_dynamic_mul", "js_dynamic_mod"] {
        assert!(
            ir.contains(&format!("call double @{helper}(")),
            "the arithmetic subtree must retain {helper}:\n{ir}"
        );
    }
}
