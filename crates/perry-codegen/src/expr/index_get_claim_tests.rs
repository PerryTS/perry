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

/// The ordered block labels of ONE dynamic element-read site, with the
/// per-site numeric suffix stripped.
fn dynamic_index_site_blocks(ir: &str) -> Vec<String> {
    ir.lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(|line| line.trim_end().strip_suffix(':'))
        .filter(|label| label.starts_with("arrlike.") || label.starts_with("tav."))
        .map(|label| {
            label
                .rsplit_once('.')
                .filter(|(_, suffix)| suffix.chars().all(|c| c.is_ascii_digit()))
                .map_or(label.to_string(), |(head, _)| head.to_string())
        })
        .collect()
}

/// #T2 ("inline hit, one exit"): the emitted `obj[i]` for an erased receiver
/// keeps exactly two inline hits — the packed ordinary-Array arm and the
/// object-backed MRU cache hit — and routes everything else through ONE
/// runtime call.
///
/// This replaces `unknown_numeric_read_guards_dense_subclass_families_and_
/// spilled_length`, which pinned the tower those arms used to be inlined
/// into (eight typed-array kind arms behind a seven-block kind dispatch, the
/// dense-tail family-token tier, the spilled-`length` and spilled-element
/// tiers, the elements-backed subclass probe and the lazy-JSON-array probe —
/// ~50 blocks and ~316 pre-RS4GC instructions per site, 55% of all IR on
/// `prettier/plugins/flow.mjs`). Each of those was an acceleration of a
/// decision `js_packed_arraylike_index_get` already makes, and the exit still
/// calls it with this site's own cache slot, so neither the answer nor the
/// primed cache words moved.
#[test]
fn unknown_numeric_read_is_one_inline_hit_and_one_out_of_line_exit() {
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
    // The complete emitted shape of one site, in order. A change here is a
    // change to the per-site code-size contract and must be measured, not
    // waved through.
    assert_eq!(
        dynamic_index_site_blocks(&ir),
        vec![
            "arrlike.ic.header",
            "arrlike.ic.brand",
            "arrlike.ic.array_guard",
            "arrlike.ic.array_load",
            "tav.brand",
            "tav.kind_guard",
            "tav.width",
            "tav.width4",
            "tav.width2",
            "tav.w8",
            "tav.w4",
            "tav.w2",
            "tav.w1",
            "arrlike.elem.kind",
            "arrlike.elem.meta",
            "arrlike.elem.store",
            "arrlike.elem.bounds",
            "arrlike.elem.load",
            "arrlike.elem.value",
            "arrlike.ic.miss",
            "arrlike.ic.merge",
        ],
        "the dynamic element read must emit exactly the inline hit plus one exit:\n{ir}"
    );
    // Exactly one runtime call for the whole site, and it is the exit.
    assert_eq!(
        ir.matches("call double @js_packed_arraylike_index_get(")
            .count(),
        1,
        "the site must have exactly one out-of-line edge:\n{ir}"
    );
    for absent in [
        // the per-kind typed-array ladder, collapsed onto element width
        "tav.get.brand",
        "tav.k.i8",
        "tav.k.f64",
        "tav.kd1",
        // the whole shape-carried Array-subclass IC tower, which cannot hit
        // while the elements store is the default representation
        "arrlike.ic.shape",
        "arrlike.ic.identity",
        "arrlike.ic.exact",
        "arrlike.ic.family_meta",
        "arrlike.ic.family_token",
        "arrlike.ic.bounds",
        "arrlike.ic.length_inline",
        "arrlike.ic.length_spill_meta",
        "arrlike.ic.length_spill_ptr",
        "arrlike.ic.length_spill_load",
        "arrlike.ic.range",
        "arrlike.ic.inline",
        "arrlike.ic.spill_or_miss",
        "arrlike.ic.spill_ptr",
        "arrlike.ic.spill_load",
        // the lazy-JSON-array tier
        "arrlike.lazy.kind",
        "arrlike.lazy.call",
        // and the runtime entries only those arms called
        "js_lazy_array_index_probe",
        "js_dyn_index_get",
        "js_number_coerce",
    ] {
        assert!(
            !ir.contains(absent),
            "`{absent}` must no longer be emitted at a dynamic element-read site:\n{ir}"
        );
    }
    // The inline hits themselves: a guarded ordinary-Array element load, and
    // the elements-backed Array-subclass probe's own load.
    let array_load = super::class_field_barrier_tests::block_body(&ir, "arrlike.ic.array_load.")
        .expect("the ordinary-Array load block exists");
    assert!(
        array_load.contains("load double") && array_load.contains("select i1"),
        "the packed Array hit must stay a direct load with an inline hole->undefined:\n{array_load}"
    );
    let store = super::class_field_barrier_tests::block_body(&ir, "arrlike.elem.store.")
        .expect("the elements-store probe block exists");
    assert!(
        store.contains("getelementptr i64, ptr %") && store.contains(", i64 12"),
        "the probe must load ObjectMeta.elements at word 12:\n{store}"
    );
    let elem_load = super::class_field_barrier_tests::block_body(&ir, "arrlike.elem.load.")
        .expect("the elements-store load block exists");
    assert!(
        elem_load.contains("load double") && !elem_load.contains("call "),
        "the elements-backed hit must load the element with no runtime call:\n{elem_load}"
    );
    // Nothing inline reads the site's cache any more; only the exit mentions
    // it, and only as an ADDRESS — a link-time constant needing no load. (The
    // tier that used to load word 0 through `emit_inline_cache_slot` is the
    // shape-carried tower, now behind the exit.)
    for block in dynamic_index_site_blocks(&ir) {
        let body = super::class_field_barrier_tests::block_body(&ir, &format!("{block}."))
            .unwrap_or_else(|| panic!("{block} block exists"));
        if block == "arrlike.ic.miss" {
            assert!(
                body.contains("ptr @perry_ic_") && !body.contains("load ptr, ptr @perry_ic_"),
                "the exit must take the slot's address, not its contents:\n{body}"
            );
        } else {
            assert!(
                !body.contains("@perry_ic_"),
                "no inline arm may touch the site's cache slot ({block}):\n{body}"
            );
        }
    }
}

/// In a number context every arm's value must be a Number, or the merge phi
/// is not uniformly one. The coercion is therefore COUPLED: each inline hit
/// and the exit either all wrap their result in `js_number_coerce` or none
/// does.
///
/// The exit deliberately does NOT fold that coercion into a flag argument of
/// its own (a fourth parameter on `js_packed_arraylike_index_get`): the extra
/// argument and its test are paid by every receiver that reaches the exit,
/// and measured +0.43% retired instructions on `object_deep_clone`, +0.18% on
/// `json_parse_1mb` and +0.13% on `batch` — to spare a `js_number_coerce`
/// from an arm that no TypeScript fixture can reach.
///
/// `coerce_slow_to_number = true` is currently **unreachable from
/// TypeScript**, on `main` as well as here: `lower_binary` routes `-`/`*`/`/`
/// with an unproven operand to `lower_guarded_numeric_arith` and the bitwise
/// ops to the `ToInt32` lowering, so `lower_arithmetic_operand` — the only
/// caller of `lower_unknown_local_index_get_for_number_context` — is not
/// reached by an erased-receiver element read. Measured: no
/// `js_number_coerce` is emitted inside the site blocks of any of eleven
/// probe shapes (`a[i] * 2`, `a[i] ^ 0`, `a[i] - 1`, `a[i] | 0`, `2 - a[i]`,
/// `a[i] << 1`, `a[i] * a[i]`, a loop accumulator, and the declared-array
/// claim forms `a[b[i]] - 1`, `a[i] * 3`, `a[k] - 1`) under the base
/// toolchain either. This test therefore pins the COUPLING rather than a
/// literal expectation: it fails the moment a site coerces on one arm and not
/// another.
#[test]
fn the_number_context_coercion_is_coupled_across_every_arm() {
    for name in [
        "unknown_dense_subclass_read",
        "unknown_dense_subclass_read_number_context",
    ] {
        let ir = ir_for(
            name,
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
                    ty: Type::Number,
                    mutable: false,
                    init: Some(Expr::Binary {
                        op: BinaryOp::Sub,
                        left: Box::new(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(ITEMS)),
                            index: Box::new(Expr::Integer(0)),
                        }),
                        right: Box::new(Expr::Number(1.0)),
                    }),
                },
            ],
        );
        assert_eq!(
            dynamic_index_site_blocks(&ir).len(),
            21,
            "{name}: a number context must not change the emitted block shape:\n{ir}"
        );
        let miss = super::class_field_barrier_tests::block_body(&ir, "arrlike.ic.miss.")
            .unwrap_or_else(|| panic!("{name}: the exit block exists"));
        assert!(
            miss.contains("call double @js_packed_arraylike_index_get("),
            "{name}: the exit must be the dispatcher call:\n{miss}"
        );
        let coerces = miss.contains("call double @js_number_coerce(");
        assert_eq!(
            miss.matches("call ").count(),
            if coerces { 2 } else { 1 },
            "{name}: the exit is one receiver classification, plus a ToNumber only \
             in a number context:\n{miss}"
        );
        for hit in ["arrlike.ic.array_load.", "arrlike.elem.value."] {
            let body = super::class_field_barrier_tests::block_body(&ir, hit)
                .unwrap_or_else(|| panic!("{name}: {hit} block exists"));
            assert_eq!(
                body.contains("call double @js_number_coerce("),
                coerces,
                "{name}: {hit} must coerce exactly when the exit does, or the merge \
                 phi is not uniformly a Number:\n{body}"
            );
        }
    }
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

/// #T2: a typed-array receiver behind an erased type leaves through the
/// site's single exit instead of the eight inline element-kind arms.
///
/// This replaces `unknown_numeric_read_brands_typed_arrays_off_the_header_
/// not_the_kind_cache`, which pinned that inlined ladder. The brand is still
/// read off the managed `GcHeader` and never from the 64-slot direct-mapped
/// `PERRY_TA_KIND_CACHE` — the runtime exit reads the `TypedArrayHeader`
/// itself, exactly as the inline arms did — so the #5525 property that made
/// them worth inlining is intact; only their per-site code is gone.
#[test]
fn unknown_numeric_read_routes_typed_arrays_through_the_single_exit() {
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
    // The site reads ONE managed-header brand byte, and it selects the
    // ordinary-Array arm; every other `obj_type` — typed arrays included —
    // continues to the object arm's own `GC_TYPE_OBJECT` test and, failing
    // that, to the exit.
    let header = super::class_field_barrier_tests::block_body(&ir, "arrlike.ic.header.")
        .expect("the managed-header block exists");
    assert!(
        header.contains("load i8") && header.contains(", 1\n"),
        "the site must brand the receiver off its GcHeader:\n{header}"
    );
    // The typed-array arm is back inline (measured: out of line it cost a
    // dynamically-typed `Float64Array` sum +122.9% walltime / +206.3%
    // instructions), but collapsed onto the ELEMENT WIDTH the header stores
    // rather than the element KIND: four load blocks, not eight behind a
    // seven-block dispatch.
    for width in ["tav.w1", "tav.w2", "tav.w4", "tav.w8"] {
        assert!(
            ir.contains(width),
            "the width-collapsed typed-array arm must emit `{width}`:\n{ir}"
        );
    }
    for gone in ["tav.get.", "tav.k.", "tav.kd"] {
        assert!(
            !ir.contains(gone),
            "the per-kind ladder `{gone}` must not come back:\n{ir}"
        );
    }
    // #10118: the brand test decides on the TAG ALONE. Everything an
    // Array-subclass or `JSON.parse` receiver would otherwise compute before
    // failing it — the view guard, the element kind, the bounds check — sits
    // behind the tag in `tav.kind_guard`.
    let ta_brand = super::class_field_barrier_tests::block_body(&ir, "tav.brand.")
        .expect("the typed-array brand block exists");
    assert!(
        ta_brand.contains(", 11") && ta_brand.contains("arrlike.elem.kind"),
        "the arm must test GC_TYPE_TYPED_ARRAY and decline to the object arm:\n{ta_brand}"
    );
    assert_eq!(
        (
            ta_brand.matches("icmp ").count(),
            ta_brand.matches("load ").count(),
            ta_brand.matches("and i1").count()
        ),
        (1, 0, 0),
        "the brand test must be ONE compare on the already-loaded tag — no kind \
         load, no view-guard load, no AND-reduction:\n{ta_brand}"
    );
    let ta_kind_guard = super::class_field_barrier_tests::block_body(&ir, "tav.kind_guard.")
        .expect("the typed-array kind/bounds guard exists");
    assert!(
        ta_kind_guard.contains("@PERRY_TA_VIEW_GUARD") && ta_kind_guard.contains("arrlike.ic.miss"),
        "inline storage, the element kind and the bounds check belong behind the \
         tag, and their miss leaves through the single exit:\n{ta_kind_guard}"
    );
    let w4 = super::class_field_barrier_tests::block_body(&ir, "tav.w4.")
        .expect("the 4-byte width block exists");
    assert_eq!(
        w4.matches("load ").count(),
        1,
        "Int32Array/Uint32Array/Float32Array must resolve from ONE load:\n{w4}"
    );
    assert_eq!(
        w4.matches("select ").count(),
        2,
        "signedness and the float form must be `select`s, not branches:\n{w4}"
    );
    assert!(
        ir.contains("call double @js_packed_arraylike_index_get("),
        "a BigInt/Float16 lane, a live view or an out-of-bounds typed-array read \
         must still reach the single exit:\n{ir}"
    );
    assert!(
        !ir.contains("@PERRY_TA_KIND_CACHE"),
        "neither the site nor its exit may depend on the kind cache:\n{ir}"
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
        ir.contains("arrlike.ic.header") && ir.contains("arrlike.elem.store"),
        "an integer key must reach the inline element-read hit:\n{ir}"
    );
    // Only an ordinary `ObjectHeader` has the words the MRU cache hit reads,
    // so the object arm must re-test `GC_TYPE_OBJECT` itself: the brand block
    // only proves "not GC_TYPE_ARRAY". Native Buffers, typed arrays, lazy
    // JSON arrays and every other exotic managed cell must leave through the
    // exit BEFORE that load — reading their header word at offset 0/4 as a
    // `(class_id, ShapeId)` identity would compare garbage.
    let kind = super::class_field_barrier_tests::block_body(&ir, "arrlike.elem.kind.")
        .expect("the object-kind guard exists");
    assert!(
        kind.contains("icmp eq i8") && kind.contains(", 2") && kind.contains("arrlike.ic.miss"),
        "only GC_TYPE_OBJECT may reach the ObjectMeta.elements load; everything \
         else must leave through the single exit:\n{kind}"
    );
    // The elements-backed subclass probe, the lazy-JSON-array probe and the
    // dense-tail family token now live behind that exit rather than at every
    // read site.
    let miss = super::class_field_barrier_tests::block_body(&ir, "arrlike.ic.miss.")
        .expect("the exit block exists");
    assert!(
        miss.contains("call double @js_packed_arraylike_index_get("),
        "the exit must be one call:\n{miss}"
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
            && ir.matches("arrlike.elem.store").count() >= 2
            && ir.matches("call double @js_packed_arraylike_index_get(").count() >= 2,
        "every other heap receiver must reach the inline element-read hit and its single exit from BOTH the canonical and the runtime-key arm:\n{ir}"
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
