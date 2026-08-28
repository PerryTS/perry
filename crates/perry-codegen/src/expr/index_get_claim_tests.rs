//! #7891: an array annotation is a claim, not a receiver-tag proof.
//!
//! These IR assertions discriminate the fix from a parity-only test: a string
//! key must retain the SSO receiver representation, while the numeric sibling
//! must keep the guarded array tier whose receiver checks make that claim safe.

use crate::temp_root_coverage::main_ir_for as ir_for;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, Param, Stmt};

const ITEMS: u32 = 1;
const RESULT: u32 = 2;
const SYMBOL: u32 = 3;
const KEY: u32 = 4;

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

/// An `Any`-typed dynamic key (`packed[sparse[x]]`, `a[b[i]]`) on a
/// declared-array receiver is tested inline for "integer-valued double in
/// [0, 2^32)"; a hit takes the same receiver-unknown numeric tiers a
/// statically proven index takes (inline typed-array read → dense
/// Array-subclass `arrlike.ic` → complete dispatcher), while every other key
/// keeps the out-of-line `js_array_get_index_or_string` route.
#[test]
fn any_typed_dynamic_key_takes_the_numeric_tiers_when_it_is_an_array_index() {
    const SPARSE: u32 = 41;
    let ir = ir_for(
        "any_key_index_read",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "packed".to_string(),
                ty: Type::Array(Box::new(Type::Any)),
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
                id: SPARSE,
                name: "sparse".to_string(),
                ty: Type::Array(Box::new(Type::Any)),
                mutable: false,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(0.0)]),
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
                    index: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(SPARSE)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                }),
            },
        ],
    );
    assert!(
        ir.contains("aidxkey.int.exact") && ir.contains("aidxkey.generic"),
        "the dynamic key must be classified inline before choosing a route:\n{ir}"
    );
    let exact = super::class_field_barrier_tests::block_body(&ir, "aidxkey.int.")
        .expect("the range-checked key block exists");
    assert!(
        exact.contains("fptosi double")
            && exact.contains("sitofp i64")
            && exact.contains("fcmp oeq"),
        "the integer test must be the fptosi/sitofp round trip:\n{exact}"
    );
    assert!(
        ir.contains("tav.get.brand") && ir.contains("arrlike.ic.family_token"),
        "an integer key must reach the inline typed-array and dense-subclass tiers:\n{ir}"
    );
    // The elements-backed subclass probe sits ahead of the shape IC: meta
    // word → `ObjectMeta.elements` (word 12) → inner-array bounds → slot.
    let store = super::class_field_barrier_tests::block_body(&ir, "arrlike.elem.store.")
        .expect("the elements-store probe block exists");
    assert!(
        store.contains("getelementptr i64, ptr %") && store.contains(", i64 12"),
        "the probe must load ObjectMeta.elements at word 12:\n{store}"
    );
    assert!(
        ir.contains("arrlike.elem.bounds") && ir.contains("arrlike.elem.load"),
        "the probe must bounds-check and load from the inner array:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_array_get_index_or_string("),
        "non-index keys must keep the complete key route:\n{ir}"
    );
}

/// The canonical-i32 arm of the same `packed[sparse[x]]` site: an erased
/// Array declaration admits object-backed Array subclasses, so a canonical
/// integer key must not be committed to the guarded plain-array tier — whose
/// feedback fallback classifies the receiver out of line on every read (the
/// 2.2× wolf-ecs regression after #8872). The element arm brands the
/// receiver once and sends non-`GC_TYPE_ARRAY` heap pointers to the
/// receiver-unknown numeric tiers instead.
#[test]
fn claimed_array_receiver_brands_before_committing_a_canonical_key_to_the_plain_tier() {
    const SPARSE: u32 = 41;
    let ir = ir_for(
        "claimed_receiver_brand",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "packed".to_string(),
                ty: Type::Array(Box::new(Type::Any)),
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
                id: SPARSE,
                name: "sparse".to_string(),
                ty: Type::Array(Box::new(Type::Any)),
                mutable: false,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(0.0)]),
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
                    index: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(SPARSE)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                }),
            },
        ],
    );
    assert!(
        ir.contains("aidx.canonical") && ir.contains("aidx.claimed.brand"),
        "the canonical-i32 arm must brand the claimed receiver before the plain tier:\n{ir}"
    );
    let brand = super::class_field_barrier_tests::block_body(&ir, "aidx.claimed.brand")
        .expect("the brand block exists");
    assert!(
        brand.contains("load i8, ptr") && brand.contains("icmp eq i8") && brand.contains(", 1"),
        "the brand block must read the GcHeader type byte and test GC_TYPE_ARRAY:\n{brand}"
    );
    assert!(
        ir.contains("aidx.claimed.array") && ir.contains("aidx.dynamic.fast"),
        "a plain Array keeps the guarded element tier:\n{ir}"
    );
    assert!(
        ir.contains("aidx.claimed.other")
            && ir.matches("arrlike.ic.family_token").count() >= 2
            && ir.matches("tav.get.brand").count() >= 2,
        "every other heap receiver must reach the inline typed-array and dense-subclass tiers from BOTH the canonical and the runtime-key arm:\n{ir}"
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
