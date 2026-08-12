//! #7891: an array annotation is a claim, not a receiver-tag proof.
//!
//! These IR assertions discriminate the fix from a parity-only test: a string
//! key must retain the SSO receiver representation, while the numeric sibling
//! must keep the guarded array tier whose receiver checks make that claim safe.

use crate::temp_root_coverage::main_ir_for as ir_for;
use perry_hir::types::Type;
use perry_hir::{Expr, Stmt};

const ITEMS: u32 = 1;
const RESULT: u32 = 2;

fn declared_array_read_ir(name: &str, index: Expr) -> String {
    ir_for(
        name,
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                // Deliberately violate the annotation in HIR. The source-level
                // repro does the same through an `any` value stored in a typed
                // field; this smaller shape reaches the identical IndexGet arm.
                init: Some(Expr::String("ss".to_string())),
            },
            Stmt::Let {
                id: RESULT,
                name: "result".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(ITEMS)),
                    index: Box::new(index),
                }),
            },
        ],
    )
}

#[test]
fn string_key_on_a_declared_array_keeps_the_receiver_boxed() {
    let ir = declared_array_read_ir("declared_array_string_key", Expr::String("0".to_string()));
    assert!(
        ir.contains("aidxkey.sso") && ir.contains("call double @js_string_index_get_boxed("),
        "the claim-safe SSO tag arm was not emitted:\n{ir}"
    );
    assert!(
        ir.contains("aidxkey.raw") && ir.contains("call double @js_array_get_index_or_string("),
        "the pointer/primitive receiver fallback disappeared:\n{ir}"
    );
}

#[test]
fn numeric_key_on_a_declared_array_keeps_the_guarded_array_tier() {
    let ir = declared_array_read_ir("declared_array_numeric_key", Expr::Integer(0));
    assert!(
        ir.contains("arr.guard.deref"),
        "the numeric receiver-validation tier was not emitted:\n{ir}"
    );
    assert!(
        !ir.contains("aidxkey.sso") && !ir.contains("call double @js_string_index_get_boxed("),
        "the SSO receiver guard widened onto the numeric array path:\n{ir}"
    );
}
