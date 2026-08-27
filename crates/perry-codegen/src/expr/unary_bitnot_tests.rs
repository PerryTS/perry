//! Runtime-proof boundary for native unary bitwise-NOT lowering.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Stmt, UnaryOp};

use crate::temp_root_coverage::main_ir_for as ir_for;

const X: u32 = 1;
const Y: u32 = 2;
const RESULT: u32 = 3;

fn erased(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Undefined),
    }
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

fn bitnot(operand: Expr) -> Expr {
    Expr::Unary {
        op: UnaryOp::BitNot,
        operand: Box::new(operand),
    }
}

#[test]
fn double_bitnot_of_coercive_number_result_stays_native() {
    let quotient = Expr::Binary {
        op: BinaryOp::Div,
        left: Box::new(Expr::LocalGet(X)),
        right: Box::new(Expr::Integer(32)),
    };
    let ir = ir_for(
        "native_double_bitnot",
        vec![erased(X, "x"), result(bitnot(bitnot(quotient)))],
    );

    assert!(
        ir.contains("call double @js_dynamic_div("),
        "the erased division must retain ToNumeric and mixed-BigInt behavior:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_bitnot("),
        "a successfully returned division result is necessarily a Number:\n{ir}"
    );
    assert!(
        ir.matches("xor i32").count() >= 2,
        "both bitwise-NOT operators should lower natively:\n{ir}"
    );
}

#[test]
fn erased_direct_bitnot_retains_bigint_dispatch() {
    let ir = ir_for(
        "dynamic_erased_bitnot",
        vec![erased(X, "x"), result(bitnot(Expr::LocalGet(X)))],
    );
    assert!(
        ir.contains("call double @js_dynamic_bitnot("),
        "an erased operand can be a BigInt and must preserve its tag:\n{ir}"
    );
}

#[test]
fn potentially_bigint_binary_result_retains_bitnot_dispatch() {
    let unknown_value = |id| Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: "value".to_string(),
    };
    let dynamic_and = Expr::Binary {
        op: BinaryOp::BitAnd,
        left: Box::new(unknown_value(X)),
        right: Box::new(unknown_value(Y)),
    };
    let ir = ir_for(
        "dynamic_nested_bigint_bitnot",
        vec![erased(X, "x"), erased(Y, "y"), result(bitnot(dynamic_and))],
    );
    assert!(
        ir.contains("call double @js_dynamic_bitand(")
            && ir.contains("call double @js_dynamic_bitnot("),
        "a both-erased bitwise chain may produce BigInt and must stay dynamic:\n{ir}"
    );
}
