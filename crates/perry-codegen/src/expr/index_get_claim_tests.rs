//! #7891: an array annotation is a claim, not a receiver-tag proof.
//!
//! These IR assertions discriminate the fix from a parity-only test: a string
//! key must retain the SSO receiver representation, while the numeric sibling
//! must keep the guarded array tier whose receiver checks make that claim safe.

use crate::temp_root_coverage::main_ir_for as ir_for;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

const ITEMS: u32 = 1;
const RESULT: u32 = 2;
const KEY: u32 = 3;

fn declared_array_read_ir(name: &str, index: Expr) -> String {
    ir_for(
        name,
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                // Deliberately violate the annotation through a dynamic
                // property read.  The initializer really evaluates to a
                // String, but (unlike a literal initializer) supplies no
                // compile-time representation proof, matching the source
                // repro's `any` value stored in a typed field.
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::String("ss".to_string()),
                    )])),
                    property: "value".to_string(),
                    byte_offset: 0,
                }),
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

fn dynamic_key_read_ir(name: &str, key_type: Type) -> String {
    let param = |id, name: &str, ty| Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let mut module = Module::new(name);
    module.functions.push(Function {
        id: 10,
        name: "read".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(ITEMS, "items", Type::Array(Box::new(Type::Any))),
            param(KEY, "key", key_type),
        ],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ITEMS)),
            index: Box::new(Expr::LocalGet(KEY)),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .expect("dynamic key fixture compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

#[test]
fn dynamic_number_key_splits_canonical_indices_from_exact_property_keys() {
    // A number parameter has no compile-time integral/range proof.
    let ir = dynamic_key_read_ir("declared_array_dynamic_number_key.ts", Type::Number);

    assert!(
        ir.contains("aidx.canonical") && ir.contains("aidx.dynamic.guard.deref"),
        "the runtime-proven canonical-index guarded tier was not emitted:\n{ir}"
    );
    assert!(
        ir.contains("aidx.runtime_key")
            && ir.contains("call double @js_array_get_index_or_string("),
        "the exact noncanonical property-key fallback disappeared:\n{ir}"
    );
    assert!(
        ir.contains("select i1") && ir.contains("fptosi double"),
        "the poison-safe range sanitization before fptosi was not emitted:\n{ir}"
    );
}

#[test]
fn generic_key_recovers_numeric_elements_without_losing_claim_safe_fallback() {
    let ir = dynamic_key_read_ir("declared_array_generic_key.ts", Type::Any);

    assert!(
        ir.contains("aidx.canonical") && ir.contains("aidx.dynamic.guard.deref"),
        "the generic key's runtime-proven numeric tier was not emitted:\n{ir}"
    );
    assert!(
        ir.contains("aidx.runtime_key")
            && ir.contains("aidxkey.sso")
            && ir.contains("call double @js_string_index_get_boxed(")
            && ir.contains("call double @js_array_get_index_or_string("),
        "the generic key lost its exact boxed-receiver fallback:\n{ir}"
    );
}
