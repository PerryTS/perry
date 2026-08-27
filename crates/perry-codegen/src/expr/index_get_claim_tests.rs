//! #7891: an array annotation is a claim, not a receiver-tag proof.
//!
//! These IR assertions discriminate the fix from a parity-only test: a string
//! key must retain the SSO receiver representation, while the numeric sibling
//! must keep the guarded array tier whose receiver checks make that claim safe.

use crate::temp_root_coverage::main_ir_for as ir_for;
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Stmt};

const ITEMS: u32 = 1;
const RESULT: u32 = 2;
const SYMBOL: u32 = 3;

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
        ir.contains("arr.guard.oob") && ir.contains("9222246136947933185"),
        "a structurally-proven OOB ordinary-array read must return the undefined tag inline:\n{ir}"
    );
    assert!(
        !ir.contains("aidxkey.sso") && !ir.contains("call double @js_string_index_get_boxed("),
        "the SSO receiver guard widened onto the numeric array path:\n{ir}"
    );
}

#[test]
fn numeric_layout_oob_array_read_returns_undefined_inline() {
    let ir = ir_for(
        "numeric_layout_oob_array_read",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![]),
                    )])),
                    property: "value".to_string(),
                    byte_offset: 0,
                }),
            },
            Stmt::Let {
                id: RESULT,
                name: "result".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(ITEMS)),
                        index: Box::new(Expr::Integer(7)),
                    }),
                    right: Box::new(Expr::Number(1.0)),
                }),
            },
        ],
    );
    assert!(
        ir.contains("arr.guard.oob") && ir.contains("9222246136947933185"),
        "a numeric-layout OOB read must inline the undefined tag:\n{ir}"
    );
    assert!(
        ir.contains("arr.guard.numeric_in_bounds"),
        "only the in-bounds arm may require the numeric element-layout proof:\n{ir}"
    );
}

#[test]
fn unknown_numeric_read_guards_dense_subclass_families_and_spilled_length() {
    let ir = ir_for(
        "unknown_dense_subclass_read",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Any,
                mutable: false,
                // Hide the representation behind an ordinary property read
                // so scalar replacement cannot fold the indexed access.
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(7.0)]),
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
                    index: Box::new(Expr::Integer(0)),
                }),
            },
        ],
    );
    assert!(
        ir.contains("arrlike.ic.family_token"),
        "the generated IC must compare the move-stable dense-tail family token:\n{ir}"
    );
    assert!(
        ir.contains("arrlike.ic.array_guard") && ir.contains("arrlike.ic.array_load"),
        "an ordinary Array behind the erased receiver must retain a direct guarded load:\n{ir}"
    );
    assert!(
        ir.contains("arrlike.ic.length_spill_load"),
        "an Array-subclass whose length slot spilled must retain an inline IC tier:\n{ir}"
    );
    assert!(
        ir.contains("arrlike.ic.range") && ir.contains("arrlike.ic.miss"),
        "the live length and cached dense-prefix bound must retain a semantic side exit:\n{ir}"
    );
}

fn dynamic_symbol_access_ir(symbol_init: Expr, field: Option<&str>) -> String {
    let symbol_read = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(ITEMS)),
        index: Box::new(Expr::LocalGet(SYMBOL)),
    };
    let result = match field {
        Some(property) => Expr::PropertyGet {
            object: Box::new(symbol_read),
            property: property.to_string(),
            byte_offset: 0,
        },
        None => symbol_read,
    };
    ir_for(
        "dynamic_symbol_read",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Any,
                mutable: false,
                // Hide the receiver behind a generic read so this exercises
                // the erased-receiver IndexGet dispatcher used by wolf-ecs.
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Object(vec![]),
                    )])),
                    property: "value".to_string(),
                    byte_offset: 0,
                }),
            },
            Stmt::Let {
                id: SYMBOL,
                name: "componentData".to_string(),
                ty: Type::Symbol,
                mutable: false,
                init: Some(symbol_init),
            },
            Stmt::Let {
                id: RESULT,
                name: "result".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(result),
            },
        ],
    )
}

fn dynamic_symbol_read_ir(symbol_init: Expr) -> String {
    dynamic_symbol_access_ir(symbol_init, None)
}

#[test]
fn proven_symbol_key_skips_registry_probe_and_uses_weak_own_property_ic() {
    let ir = dynamic_symbol_read_ir(Expr::SymbolNew(None));
    assert!(
        ir.contains("symic.hit")
            && ir.contains("load atomic i64, ptr @PERRY_SYMBOL_PROPERTY_IC_EPOCH acquire")
            && ir.contains("call double @js_object_get_symbol_property_ic_miss("),
        "the weak epoch-guarded Symbol property IC was not emitted:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_is_symbol("),
        "compiler-owned Symbol provenance must remove the registry probe:\n{ir}"
    );
}

#[test]
fn proven_symbol_then_named_field_composes_identity_and_shape_caches() {
    let ir = dynamic_symbol_access_ir(Expr::SymbolNew(None), Some("id"));
    assert!(
        ir.contains("symfield.identity")
            && ir.contains("symfield.hit")
            && ir.contains("load atomic i64, ptr @PERRY_SYMBOL_PROPERTY_IC_EPOCH acquire")
            && ir.contains("call double @js_object_get_symbol_then_field_ic_miss(")
            && ir.contains("4611686018427387904"),
        "the weak Symbol identity and exact ShapeId field caches were not composed:\n{ir}"
    );
    assert!(
        ir.contains("and i16") && ir.contains("2048"),
        "the composed hit must reject descriptor-bearing metadata objects:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_is_symbol("),
        "compiler-owned Symbol provenance must retain its registry-free route:\n{ir}"
    );
}

#[test]
fn erased_symbol_annotation_does_not_bypass_runtime_validation() {
    let ir = dynamic_symbol_read_ir(Expr::PropertyGet {
        object: Box::new(Expr::Object(vec![("value".to_string(), Expr::Number(7.0))])),
        property: "value".to_string(),
        byte_offset: 0,
    });
    assert!(
        !ir.contains("symic.hit")
            && !ir.contains("call double @js_object_get_symbol_property_ic_miss("),
        "a TypeScript Symbol annotation without initializer provenance must not enter the exact-Symbol IC:\n{ir}"
    );
}

/// The inline dynamic typed-array read brands the receiver off its managed
/// `GC_TYPE_TYPED_ARRAY` header and reads the element kind from the
/// `TypedArrayHeader` itself, instead of probing the 64-slot direct-mapped
/// `PERRY_TA_KIND_CACHE` that every ordinary-array registry miss also writes
/// negative entries into (a hot typed array kept getting evicted and missed
/// the tier on every access).
#[test]
fn unknown_numeric_read_brands_typed_arrays_off_the_header_not_the_kind_cache() {
    let ir = ir_for(
        "unknown_typed_array_read_brand",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(7.0)]),
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
                    index: Box::new(Expr::Integer(0)),
                }),
            },
        ],
    );
    assert!(
        ir.contains("tav.get.brand"),
        "the inline typed-array tier must brand the receiver off its header:\n{ir}"
    );
    let brand = super::class_field_barrier_tests::block_body(&ir, "tav.get.brand.")
        .expect("brand block exists");
    assert!(
        brand.contains("icmp eq i8") && brand.contains(", 11"),
        "the brand block must test GC_TYPE_TYPED_ARRAY (11):\n{brand}"
    );
    assert!(
        brand.contains("load i8"),
        "the element kind must be read from the TypedArrayHeader:\n{brand}"
    );
    assert!(
        !ir.contains("@PERRY_TA_KIND_CACHE"),
        "the inline read must no longer depend on the kind cache:\n{ir}"
    );
}
