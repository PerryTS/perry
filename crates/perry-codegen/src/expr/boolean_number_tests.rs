//! #5497 Lever E: IR-census coverage for native Boolean-to-Number consumers.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt};

use crate::temp_root_coverage::main_ir_for as ir_for;

const FLAG: u32 = 1;
const RESULT: u32 = 2;
const LIAR: u32 = 3;

fn proven_flag() -> Stmt {
    Stmt::Let {
        id: FLAG,
        name: "flag".to_string(),
        ty: Type::Boolean,
        mutable: true,
        init: Some(Expr::Bool(true)),
    }
}

fn result(expr: Expr, ty: Type) -> Stmt {
    Stmt::Let {
        id: RESULT,
        name: "result".to_string(),
        ty,
        mutable: false,
        init: Some(expr),
    }
}

#[test]
fn proven_boolean_arithmetic_uses_native_conversion_and_ops() {
    for (name, op, native_op) in [
        ("bool_native_add", BinaryOp::Add, "fadd double"),
        ("bool_native_mul", BinaryOp::Mul, "fmul double"),
    ] {
        let ir = ir_for(
            name,
            vec![
                proven_flag(),
                result(
                    Expr::Binary {
                        op,
                        left: Box::new(Expr::LocalGet(FLAG)),
                        right: Box::new(Expr::Number(2.0)),
                    },
                    Type::Number,
                ),
            ],
        );
        assert!(
            ir.contains("uitofp i1") && ir.contains(native_op),
            "proven Boolean arithmetic should stay native:\n{ir}"
        );
        assert!(
            !ir.contains("call double @js_number_coerce(")
                && !ir.contains("call double @js_dynamic_string_or_number_add("),
            "proven Boolean arithmetic retained dynamic coercion:\n{ir}"
        );
    }
}

#[test]
fn proven_boolean_relational_compare_uses_native_fcmp() {
    let ir = ir_for(
        "bool_native_relational",
        vec![
            proven_flag(),
            result(
                Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(FLAG)),
                    right: Box::new(Expr::Number(2.0)),
                },
                Type::Boolean,
            ),
        ],
    );
    assert!(
        ir.contains("uitofp i1") && ir.contains("fcmp olt double"),
        "proven Boolean relational compare should use native fcmp:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_rel_lt("),
        "proven Boolean relational compare retained js_rel_lt:\n{ir}"
    );
}

#[test]
fn proven_boolean_compare_rejects_declared_only_nested_add() {
    let ir = ir_for(
        "bool_relational_declared_only_add",
        vec![
            proven_flag(),
            Stmt::Let {
                id: LIAR,
                name: "liar".to_string(),
                ty: Type::Number,
                mutable: false,
                // Mirrors `let liar: number = "4" as any`: annotations are
                // erased, so the slot may hold a NaN-boxed string at runtime.
                init: Some(Expr::String("4".to_string())),
            },
            result(
                Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(FLAG)),
                    right: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(LIAR)),
                        right: Box::new(Expr::Number(1.0)),
                    }),
                },
                Type::Boolean,
            ),
        ],
    );
    assert!(
        ir.contains("call double @js_rel_lt("),
        "a nested add with a declared-only Number must keep abstract relational semantics:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add("),
        "the nested add must preserve runtime string concatenation:\n{ir}"
    );
}

#[test]
fn invalidated_boolean_proof_keeps_dynamic_coercion_and_add_dispatch() {
    let ir = ir_for(
        "bool_native_proof_invalidated",
        vec![
            proven_flag(),
            Stmt::Expr(Expr::LocalSet(
                FLAG,
                Box::new(Expr::String("4".to_string())),
            )),
            result(
                Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(FLAG)),
                    right: Box::new(Expr::Number(2.0)),
                },
                Type::Any,
            ),
            Stmt::Expr(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(FLAG)),
                right: Box::new(Expr::Number(2.0)),
            }),
        ],
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add("),
        "a Boolean local invalidated by a string write must preserve concat semantics:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_mul("),
        "an invalidated Boolean proof must preserve BigInt-aware runtime ToNumeric coercion:\n{ir}"
    );
}
