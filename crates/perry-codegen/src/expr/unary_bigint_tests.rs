//! BigInt-aware unary negation routing.

use perry_hir::types::Type;
use perry_hir::{Expr, Stmt, UnaryOp};

use crate::temp_root_coverage::main_ir_for as ir_for;

const VALUES: u32 = 1;
const RESULT: u32 = 2;

#[test]
fn bigint_array_element_negation_uses_dynamic_numeric_dispatch() {
    let ir = ir_for(
        "bigint_array_element_negation",
        vec![
            Stmt::Let {
                id: VALUES,
                name: "values".to_string(),
                ty: Type::Array(Box::new(Type::BigInt)),
                mutable: false,
                init: Some(Expr::Array(vec![Expr::BigInt(
                    "18446744073709551616".to_string(),
                )])),
            },
            Stmt::Let {
                id: RESULT,
                name: "result".to_string(),
                ty: Type::BigInt,
                mutable: false,
                init: Some(Expr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(VALUES)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                }),
            },
        ],
    );

    assert!(
        ir.contains("call double @js_dynamic_neg("),
        "a BigInt array element must retain its tag and exact value:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_number_coerce("),
        "BigInt negation must use ToNumeric rather than ToNumber:\n{ir}"
    );
}
