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
                    byte_end: 0,
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
                    byte_offset: 0,
                    byte_end: 0,
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
    let call = ir
        .lines()
        .find(|l| l.contains("call double @perry_array_read_cold_"))
        .expect("one shared cold key operation");
    let operands: Vec<_> = call.split('(').nth(1).unwrap().split(',').take(2).collect();
    assert!(
        operands.len() == 2 && operands.iter().all(|v| v.trim().starts_with("double ")),
        "the claim-safe slow operation must receive the boxed receiver and key: {call}"
    );
    assert!(
        !ir.contains("sso.split.sso"),
        "the slow body must not be duplicated at the site"
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
        ir.contains("arr.fallback") && ir.contains("call double @perry_array_read_cold_shared"),
        "an OOB ordinary-array read must retain the exact shared runtime fallback:\n{ir}"
    );
    assert!(
        !ir.contains("aidxkey.sso") && !ir.contains("call double @js_string_index_get_boxed("),
        "the SSO receiver guard widened onto the numeric array path:\n{ir}"
    );
}

fn numeric_layout_oob_array_read_ir() -> String {
    ir_for(
        "numeric_layout_oob_array_read",
        vec![
            Stmt::Let {
                id: ITEMS,
                name: "items".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: false,
                init: Some(Expr::PropertyGet {
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![], None),
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
                        byte_offset: 0,
                        byte_end: 0,
                        object: Box::new(Expr::LocalGet(ITEMS)),
                        index: Box::new(Expr::Integer(7)),
                    }),
                    right: Box::new(Expr::Number(1.0)),
                }),
            },
        ],
    )
}

#[test]
fn numeric_layout_oob_array_read_uses_shared_cold_body() {
    let ir = numeric_layout_oob_array_read_ir();
    assert!(
        ir.contains("arr.fallback") && ir.contains("call double @perry_array_read_cold_"),
        "a numeric-layout OOB read must use the shared generic fallback:\n{ir}"
    );
    assert!(
        ir.contains("arr.guard.range") && !ir.contains("8388608"),
        "the numeric read must not consult the array layout bit:\n{ir}"
    );
    // The proof is the loaded element's own tag, tested inside the fast arm
    // (the consumer's copy of the predicate elsewhere does not count).
    let mut in_fast = false;
    let mut fast_arm = Vec::new();
    for line in ir.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            in_fast = line.starts_with("arr.fast.");
        } else if in_fast {
            fast_arm.push(line.trim());
        }
    }
    assert!(
        fast_arm
            .iter()
            .any(|l| l.contains("icmp ule i64") && l.contains("9221401712017801215")),
        "the fast arm must test the loaded element:\n{ir}"
    );
}

#[test]
fn instrumented_numeric_array_read_keeps_census_after_sole_admission() {
    // Census enablement is cached process-wide. Use a fresh test process so
    // this test also exercises the instrument when the suite defaults to OFF.
    const CHILD: &str = "NUMERIC_PHI_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let current = std::thread::current();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([current.name().unwrap(), "--exact", "--test-threads=1"])
            .env(CHILD, "1")
            .env("PERRY_STORE_CENSUS", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "instrumented array read failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    assert!(super::store_census::enabled());
    let ir = numeric_layout_oob_array_read_ir();
    let mut in_fast = false;
    let mut instructions = Vec::new();
    for line in ir.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            if in_fast {
                break;
            }
            in_fast = line.starts_with("arr.fast.");
        } else if in_fast && !line.trim().is_empty() {
            instructions.push(line.trim());
        }
    }
    assert!(!instructions.is_empty(), "numeric fast route absent:\n{ir}");
    assert!(
        !instructions.iter().any(|line| line.contains(" = phi i64 ")),
        "the compact path has one admitted handle, with no cold handle merge: {instructions:?}"
    );
    assert!(
        instructions
            .iter()
            .any(|line| line.contains("@PERRY_STORE_CENSUS")),
        "the fast route's census must actually be emitted: {instructions:?}"
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

/// The complete emitted shape of one dynamic element-read site, in order.
/// Shared by every test that pins it, so a new arm updates ONE list rather
/// than a count literal per test (#11588 and #11589 each bumped such a
/// literal from 21 to 24 independently; together the site has 27 blocks).
const DYNAMIC_INDEX_SITE_BLOCKS: &[&str] = &[
    // #10515: the admitted byte-view (`Uint8Array` / `Buffer`) arm.
    "arrlike.u8.brand",
    "arrlike.u8.view",
    "arrlike.u8.bounds",
    "arrlike.u8.load",
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
    "arrlike.elem_meta",
    "arrlike.elem.store",
    "arrlike.elem.bounds",
    "arrlike.elem.load",
    "arrlike.elem.value",
    // #10514: one growth-forwarding hop healed inline.
    "arrlike.ic.fwd_check",
    "arrlike.ic.fwd_follow",
    "arrlike.ic.fwd_header",
    "arrlike.ic.miss",
    "arrlike.ic.merge",
    // EK design §4: the dense-prefix hit (`index < P`) precedes the ordinary-Array
    // guard; a miss takes that guard unchanged in `array_rest`. Two blocks per site is
    // the measured code-size price of the hit (.text in reports/2026-10-22-EK-result.md).
    "arrlike.ic.prefix_hit",
    "arrlike.ic.array_rest",
    // Pointer-layout views have a separate load; owning bytes remain at +8.
    "arrlike.u8.view_load",
];

/// #T2 ("inline hit, one exit"): the emitted `obj[i]` for an erased receiver
/// keeps exactly two inline hits — the packed ordinary-Array arm and the
/// elements-backed Array-subclass load — and routes everything else through
/// ONE runtime call.
///
/// This replaces `unknown_numeric_read_guards_dense_subclass_families_and_
/// spilled_length`, which pinned the tower those arms used to be inlined
/// into (eight typed-array kind arms behind a seven-block kind dispatch, the
/// dense-tail family-token tier, the spilled-`length` and spilled-element
/// tiers, the elements-backed subclass probe and the lazy-JSON-array probe —
/// ~50 blocks and ~316 pre-RS4GC instructions per site, 55% of all IR on
/// `prettier/plugins/flow.mjs`). Each of those was an acceleration of a
/// decision `js_packed_arraylike_index_get` already makes, so the answer did
/// not move.
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
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(7.0)], None),
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
                    byte_offset: 0,
                    byte_end: 0,
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
        DYNAMIC_INDEX_SITE_BLOCKS,
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
        "call double @js_dyn_index_get(",
        "js_number_coerce",
    ] {
        assert!(
            !ir.contains(absent),
            "`{absent}` must no longer be emitted at a dynamic element-read site:\n{ir}"
        );
    }
    // The property-key dispatch arm owns the keyed holder site. It is
    // separate from the numeric tier's single packed-arraylike exit.
    let property = super::class_field_barrier_tests::block_body(&ir, "dynkey.property.")
        .expect("the property-key dispatch block exists");
    assert_eq!(
        property
            .matches("call double @perry_probe_get_keyed(")
            .count(),
        1
    );
    assert!(!property.contains("@js_packed_arraylike_index_get("));
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
    // No arm, the exit included, touches a per-site cache: the arraylike
    // index cache is deleted (object model step 5, phase 2) and the exit
    // takes only the receiver and the index.
    for block in dynamic_index_site_blocks(&ir) {
        let body = super::class_field_barrier_tests::block_body(&ir, &format!("{block}."))
            .unwrap_or_else(|| panic!("{block} block exists"));
        assert!(
            !body.contains("@perry_ic_"),
            "no arm may touch a per-site cache slot ({block}):\n{body}"
        );
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
                        byte_end: 0,
                        object: Box::new(Expr::Object(vec![(
                            "value".to_string(),
                            Expr::Array(vec![Expr::Number(7.0)], None),
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
                            byte_offset: 0,
                            byte_end: 0,
                            object: Box::new(Expr::LocalGet(ITEMS)),
                            index: Box::new(Expr::Integer(0)),
                        }),
                        right: Box::new(Expr::Number(1.0)),
                    }),
                },
            ],
        );
        assert_eq!(
            dynamic_index_site_blocks(&ir),
            DYNAMIC_INDEX_SITE_BLOCKS,
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
        // The remaining value-producing arms load a Number by construction
        // (a typed-array element of each width, and #10515's admitted byte
        // view via `uitofp`), so they never coerce in either context. The
        // forwarding hop (#10514) produces no value; it only re-enters
        // `arrlike.ic.brand`.
        for number_arm in [
            "arrlike.u8.load.",
            "tav.w8.",
            "tav.w4.",
            "tav.w2.",
            "tav.w1.",
        ] {
            let body = super::class_field_barrier_tests::block_body(&ir, number_arm)
                .unwrap_or_else(|| panic!("{name}: {number_arm} block exists"));
            assert!(
                !body.contains("call double @js_number_coerce("),
                "{name}: {number_arm} is a Number by construction and must not coerce:\n{body}"
            );
        }
        let u8_load = super::class_field_barrier_tests::block_body(&ir, "arrlike.u8.load.")
            .unwrap_or_else(|| panic!("{name}: arrlike.u8.load block exists"));
        assert!(
            u8_load.contains("uitofp i8"),
            "{name}: the byte-view arm's value must be a zero-extended byte:\n{u8_load}"
        );
    }
}

fn dynamic_symbol_access_ir(symbol_init: Expr, field: Option<&str>) -> String {
    let symbol_read = Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(ITEMS)),
        index: Box::new(Expr::LocalGet(SYMBOL)),
    };
    let result = match field {
        Some(property) => Expr::PropertyGet {
            byte_end: 0,
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
                    byte_end: 0,
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
fn proven_symbol_key_uses_the_keyed_shape_guard() {
    let ir = dynamic_symbol_read_ir(Expr::SymbolNew(None));
    assert!(
        ir.contains("call double @perry_probe_get_keyed(")
            && !ir.contains("@PERRY_SYMBOL_PROPERTY_IC_EPOCH"),
        "the Symbol read must take the keyed probe:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_is_symbol("),
        "compiler-owned Symbol provenance must remove the registry probe:\n{ir}"
    );
}

#[test]
fn erased_symbol_annotation_does_not_bypass_runtime_validation() {
    let ir = dynamic_symbol_read_ir(Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::Object(vec![("value".to_string(), Expr::Number(7.0))])),
        property: "value".to_string(),
        byte_offset: 0,
    });
    assert!(
        ir.contains("dynkey.property")
            && ir.contains("call double @perry_probe_get_keyed(")
            && !ir.contains("symic.hit")
            && !ir.contains("call double @js_object_get_symbol_property_ic_miss("),
        "a TypeScript Symbol annotation without initializer provenance must not enter the exact-Symbol IC:\n{ir}"
    );
}

/// An erased typed-array receiver uses one runtime exit. Its element kind
/// comes from the common header type byte.
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
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(7.0)], None),
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
                    byte_offset: 0,
                    byte_end: 0,
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
        ta_brand.contains(", 64") && ta_brand.contains("arrlike.elem.kind"),
        "the arm must test GC_TYPE_TYPED_ARRAY and decline to the object arm:\n{ta_brand}"
    );
    assert_eq!(
        (
            ta_brand.matches("icmp ").count(),
            ta_brand.matches("load ").count(),
            ta_brand.matches("and i1").count()
        ),
        (2, 0, 1),
        "the brand test must be a type-byte family guard on the already-loaded tag — no kind \
         load, no view-guard load, no AND-reduction:\n{ta_brand}"
    );
    let ta_kind_guard = super::class_field_barrier_tests::block_body(&ir, "tav.kind_guard.")
        .expect("the typed-array kind/bounds guard exists");
    assert!(ta_kind_guard.contains("bytes.header") && ta_kind_guard.contains("arrlike.ic.miss"),
        "canonical header resolution must stay behind the type proof and retain the single miss exit: {ta_kind_guard}");
    assert!(
        !ta_kind_guard.contains("load i8"),
        "no separate storage/kind payload loads: {ta_kind_guard}"
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
        3,
        "signedness, the #10779 NaN canonicalisation and the float form must be \
         `select`s, not branches:\n{w4}"
    );
    // #10779: the third select is the NaN canonicalisation, and it must apply
    // to the Float32Array lane only — the two integer forms cannot be NaN, so
    // canonicalising them would be pure cost. Pin the operand so a later edit
    // cannot quietly move it onto the merged value.
    assert!(
        w4.contains("fcmp uno double") && w4.contains("double 0x7FF8000000000000"),
        "the f32 lane must be canonicalised before the float-form select:\n{w4}"
    );
    assert_eq!(
        w4.matches("br ").count(),
        1,
        "the width-4 block must still end in exactly its unconditional branch \
         — no new control flow:\n{w4}"
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
/// declared-array receiver is normalized by the shared pure index helper to an "integer-valued double in
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
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(7.0)], None),
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
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(0.0)], None),
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
                    byte_offset: 0,
                    byte_end: 0,
                    object: Box::new(Expr::LocalGet(ITEMS)),
                    index: Box::new(Expr::IndexGet {
                        byte_offset: 0,
                        byte_end: 0,
                        object: Box::new(Expr::LocalGet(SPARSE)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                }),
            },
        ],
    );
    assert!(ir.contains("aidx.canonical") && ir.contains("aidx.runtime_key"));
    // The "integer-valued double in [0, 2^32)" test is the shared pure
    // normalizer (`memory(none)`, a GC leaf) and a sign test of its result.
    assert!(
        ir.contains("call i32 @perry_array_index_normalize_i32(") && ir.contains("icmp sge i32 %"),
        "the dynamic key must be normalized by the shared index helper:\n{ir}"
    );
    assert!(ir.contains("aidx.dynamic.guard.deref") && ir.contains("aidx.dynamic.fast"));
    assert!(ir.contains("call double @perry_array_read_cold_"));
    assert!(
        !ir.contains("arrlike.elem.store"),
        "claimed-array misses must stay out of line"
    );
}

/// The canonical-i32 arm of the same `packed[sparse[x]]` site: an erased
/// Array declaration can carry other runtime shapes. One guarded plain-array
/// tier admits its own slots; every other shape uses the existing shared
/// runtime fallback without duplicating a receiver classification tower.
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
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(7.0)], None),
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
                    byte_end: 0,
                    object: Box::new(Expr::Object(vec![(
                        "value".to_string(),
                        Expr::Array(vec![Expr::Number(0.0)], None),
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
                    byte_offset: 0,
                    byte_end: 0,
                    object: Box::new(Expr::LocalGet(ITEMS)),
                    index: Box::new(Expr::IndexGet {
                        byte_offset: 0,
                        byte_end: 0,
                        object: Box::new(Expr::LocalGet(SPARSE)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                }),
            },
        ],
    );
    assert!(ir.contains("aidx.canonical") && ir.contains("aidx.dynamic.guard.deref"));
    assert!(ir.contains("33023") && ir.contains("aidx.dynamic.fast"));
    assert!(ir.contains("call double @perry_array_read_cold_shared"));
    assert!(
        !ir.contains("aidx.claimed.brand"),
        "the receiver must not be branded twice"
    );
    assert!(
        !ir.contains("arrlike.elem.store"),
        "non-Array shapes use the existing cold runtime"
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
            byte_offset: 0,
            byte_end: 0,
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
            && ir.contains("call double @perry_array_read_cold_shared("),
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
            && ir.contains("call double @perry_array_read_cold_shared("),
        "the generic key lost its exact boxed-receiver fallback:\n{ir}"
    );
}
