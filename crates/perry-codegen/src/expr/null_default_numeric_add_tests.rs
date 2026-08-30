//! #8607: IR coverage for null-defaulted dynamic counter increments.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt};

use crate::temp_root_coverage::main_ir_for as ir_for;

const VALUE: u32 = 1;
const RESULT: u32 = 2;

fn value(init: Expr) -> Stmt {
    Stmt::Let {
        id: VALUE,
        name: "value".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn result(init: Expr) -> Stmt {
    Stmt::Let {
        id: RESULT,
        name: "result".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn increment(expr: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(expr),
        right: Box::new(Expr::Integer(1)),
    }
}

#[test]
fn null_defaulted_dynamic_increment_has_guarded_numeric_fast_path() {
    let ir = ir_for(
        "null_defaulted_dynamic_increment",
        vec![
            value(Expr::String("4".to_string())),
            result(increment(Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(VALUE)),
                    right: Box::new(Expr::Null),
                }),
                then_expr: Box::new(Expr::Integer(0)),
                else_expr: Box::new(Expr::LocalGet(VALUE)),
            })),
        ],
    );

    assert!(
        ir.contains("guarded_add.numeric") && ir.contains("fadd double"),
        "the numeric value must reach the guarded inline add:\n{ir}"
    );
    assert!(
        ir.contains("guarded_add.dynamic")
            && ir.contains("call double @js_dynamic_string_or_number_add("),
        "a non-number must retain JavaScript concatenation semantics:\n{ir}"
    );
}

#[test]
fn arbitrary_dynamic_increment_is_never_assumed_numeric() {
    let ir = ir_for(
        "arbitrary_dynamic_increment",
        vec![
            value(Expr::Undefined),
            result(increment(Expr::LocalGet(VALUE))),
        ],
    );

    // #9157 gave two-leaf trees the same guarded diamond three-leaf trees
    // already had, so an `fadd` now appears here where it previously did not.
    // The invariant this test exists for is unchanged and is asserted more
    // directly below: an `Any` is never ASSUMED numeric. It may be TESTED and
    // then added, because the cold arm still performs the dynamic `+`.
    assert!(
        ir.contains("guarded_add.numeric") == ir.contains("fadd double"),
        "an fadd on an Any value must be guarded, never bare:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add("),
        "an unguarded Any value must keep JavaScript `+` dispatch:\n{ir}"
    );
}
