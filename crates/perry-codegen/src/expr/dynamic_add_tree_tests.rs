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

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
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
fn two_leaf_dynamic_add_stays_on_direct_dispatch() {
    let mut body = dynamic_locals();
    body.push(result(add(Expr::LocalGet(A), Expr::LocalGet(B))));
    let ir = ir_for("two_leaf_dynamic_add", body);

    assert!(
        !ir.contains("guarded_add.numeric") && !ir.contains("fadd double"),
        "a single dynamic add should not pay for a separate guard diamond:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_dynamic_string_or_number_add(")
            .count(),
        1,
        "a single dynamic add should keep direct dispatch:\n{ir}"
    );
}
