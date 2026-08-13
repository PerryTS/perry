//! Rooting coverage for the computed-store arms repaired in slice 4 (#7637,
//! #7638, #7639) and the remaining #7640 read/write windows, built from HIR
//! rather than from TypeScript.
//!
//! # Why these are unit tests and not gap tests
//!
//! Counted, not assumed. Over the whole `gc-root-dominance` curated corpus (129
//! sources, 149 modules) `js_array_set_length_strict` is **called zero times**,
//! and the two arms that are reached — `js_typed_feedback_array_set_string_key`
//! and `js_typed_feedback_object_set_index_polymorphic` — are reached once each,
//! by a source whose operands provably cannot collect, so the emitted IR is
//! identical with and without the repair.
//!
//! Hand-written probes did not close the gap either, and the reason is
//! structural rather than a failure of imagination: an ordinary
//! `o.k = v` / `o[k] = v` assignment statement lowers to `Expr::PutValueSet`
//! and reaches the dynamic IC (`js_put_value_set_dyn_ic`), NOT the
//! `Expr::PropertySet` / `Expr::IndexSet` arms in these two modules. Six probe
//! shapes were tried — module-const and parameter receivers, string / symbol /
//! `any` keys, inside and outside a function — and every one of them routed
//! past the subject. That is slice 3's finding 4 again: *a branch the corpora
//! cannot reach needs a unit test, and the way to find out is to count, not to
//! read.*
//!
//! # What each test asserts, and why it cannot pass vacuously
//!
//! Each differential test builds the SAME access twice, changing one thing:
//! a later operand is either allocating or inert. The older slice-4 tests
//! compare root-slot widths; the #7640 tests additionally trace each protected
//! call operand back to its own root slot and assert that the slot store is
//! above the allocating operand while the consuming reload is below it. The
//! realloc-barrier regression at the end instead names the exact SSA head
//! consumed by its barrier.
//!
//! - `Expr::Number` ⇒ `expr_may_trigger_gc` is false ⇒ `operand_protection`
//!   returns `Reuse` ⇒ the combinator emits nothing at all, which is the
//!   property that keeps this slice's IR byte-identical on the corpus.
//! - `Expr::Object` ⇒ the window collects ⇒ the operands take slots.
//!
//! So the assertion is a DIFFERENCE between two runs of the same code path,
//! which no amount of corpus drift can turn into a tautology, and deleting the
//! operand group collapses the two counts and turns each test red. Every test
//! also asserts the arm it is about was actually reached, by name — a frame
//! width measured over a store that never got emitted would be hazard 4.

use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module as HirModule, Param, Stmt, UnaryOp, UpdateOp,
};

use super::slice8_rooting_tests::{call_operand_of, producer_line};

/// Compile a one-function module and return its LLVM IR.
fn compile_body(name: &str, body: Vec<Stmt>) -> String {
    compile_body_with_params(name, Vec::new(), body)
}

fn compile_body_with_params(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> String {
    let mut hir = HirModule::new(name);
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// How many root slots the module's code reserves.
///
/// Counted under BOTH root lowerings on purpose, because which one runs is an
/// env decision (`PERRY_RS4GC`) and a test that silently measured zero under the
/// other would be a gate that cannot fail. Under the statepoint lowering — the
/// default since #7370 — a root slot is an `alloca ptr addrspace(1)`; under the
/// shadow-stack lowering it is a `js_shadow_slot_bind`. Both arms of each
/// comparison are compiled in the same process with the same settings, so only
/// the difference is read.
fn root_slots(ir: &str) -> usize {
    ir.matches("alloca ptr addrspace(1)").count() + ir.matches("@js_shadow_slot_bind(").count()
}

/// Assert that one specific operand consumed by `callee` is stored to a native
/// root above `window_operand`'s production and reloaded from that same slot
/// below it. The tests pin native roots so this checks the actual RS4GC IR,
/// rather than allowing an unrelated allocating expression's slot to satisfy a
/// total-width comparison.
fn assert_call_operand_rooted_across_operand(
    ir: &str,
    callee: &str,
    protected_operand: usize,
    window_operand: usize,
    what: &str,
) {
    let protected = call_operand_of(ir, callee, protected_operand);
    let reload = producer_line(ir, &protected);
    let reload_line = ir.lines().nth(reload).expect("producer line exists");
    assert!(
        reload_line.contains("load ptr addrspace(1), ptr "),
        "{what}: {callee} operand {protected_operand} ({protected}) is not reloaded from a \
         native root slot after the collecting operand:\n{ir}"
    );
    let slot = reload_line
        .rsplit_once(", ptr ")
        .map(|(_, tail)| tail.split(',').next().unwrap_or(tail).trim())
        .expect("native root reload names its slot");

    let window = call_operand_of(ir, callee, window_operand);
    let window_line = producer_line(ir, &window);
    assert!(
        reload > window_line,
        "{what}: {callee} operand {protected_operand} is reloaded at line {reload}, above \
         the collecting operand produced at line {window_line}:\n{ir}"
    );

    let store = ir
        .lines()
        .enumerate()
        .take(window_line)
        .filter(|(_, line)| {
            line.contains("store ptr addrspace(1)")
                && !line.contains(" null,")
                && line
                    .rsplit_once(", ptr ")
                    .is_some_and(|(_, tail)| tail.split(',').next().unwrap_or(tail).trim() == slot)
        })
        .map(|(line, _)| line)
        .last()
        .unwrap_or_else(|| {
            panic!(
                "{what}: root slot {slot} is read below the window but has no non-null store \
                 above it:\n{ir}"
            )
        });
    assert!(
        store < window_line,
        "{what}: root store at line {store} must dominate the collecting operand at line \
         {window_line}:\n{ir}"
    );
}

/// An allocating RHS (`{ a: 1 }`) and an inert one (`1`), so each test can
/// compare the same store under a collecting and a non-collecting window.
fn allocating_value() -> Expr {
    Expr::Object(vec![("a".to_string(), Expr::Number(1.0))])
}

fn inert_value() -> Expr {
    Expr::Number(1.0)
}

fn calls(ir: &str, callee: &str) -> bool {
    let needle = format!("@{callee}(");
    ir.lines()
        .any(|line| line.contains(&needle) && !line.trim_start().starts_with("declare"))
}

/// A fixed-length Float64Array view over native-arena storage. Unlike a fresh
/// inline typed array, its cached `data_slot` can become invalid across user
/// code (arena disposal) and is not rewritten when GC rewrites side-table
/// backing pointers.
fn with_native_f64_view(tail: Stmt) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: 10,
            name: "owner".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::NativeArenaAlloc(Box::new(Expr::Integer(64)))),
        },
        Stmt::Let {
            id: 11,
            name: "view".to_string(),
            ty: Type::Named("Float64Array".to_string()),
            mutable: false,
            init: Some(Expr::NativeArenaView {
                owner: Box::new(Expr::LocalGet(10)),
                kind: perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                byte_offset: Box::new(Expr::Integer(0)),
                length: Box::new(Expr::Integer(8)),
            }),
        },
        tail,
    ]
}

/// Assert the store arm named by `callee` was emitted, then that an allocating
/// RHS costs strictly more root slots than an inert one.
fn assert_operands_rooted_only_when_the_window_collects(
    label: &str,
    callee: &str,
    build: impl Fn(Expr) -> Vec<Stmt>,
) {
    let hot = compile_body(&format!("{label}_hot"), build(allocating_value()));
    let cold = compile_body(&format!("{label}_cold"), build(inert_value()));

    // Subject liveness first: a frame width measured over a store that never
    // got emitted proves nothing (CLAUDE.md hazard 4).
    assert!(
        hot.contains(&format!("@{callee}(")),
        "{label}: the arm under test was not reached — no call to @{callee}:\n{hot}"
    );
    assert!(
        cold.contains(&format!("@{callee}(")),
        "{label}: the inert arm took a different path, so the two runs are not \
         comparable — no call to @{callee}:\n{cold}"
    );

    let hot_slots = root_slots(&hot);
    let cold_slots = root_slots(&cold);
    assert!(
        hot_slots > cold_slots,
        "{label}: an allocating RHS must put the store's operands in root slots. \
         The module reserved {hot_slots} root slots with an allocating RHS and \
         {cold_slots} with an inert one — equal means the operand group is gone \
         and the receiver (and key) are live in bare registers across the RHS \
         again."
    );
}

/// #7637 — `arr.length = f()`. The receiver is lowered first because
/// `Set(O, "length", v, true)` evaluates the reference before the value, and
/// `js_array_set_length_strict` then truncates through whatever address the
/// register still holds.
#[test]
fn arr_length_store_roots_the_receiver_across_an_allocating_rhs() {
    assert_operands_rooted_only_when_the_window_collects(
        "arr_length",
        "js_array_set_length_strict",
        |value| {
            vec![
                Stmt::Let {
                    id: 1,
                    name: "arr".to_string(),
                    ty: Type::Array(Box::new(Type::Number)),
                    mutable: false,
                    init: Some(Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)])),
                },
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "length".to_string(),
                    value: Box::new(value),
                }),
            ]
        },
    );
}

/// #7638 arm 2 — `arr[stringKey] = f()`. The key is a heap string by
/// construction on this arm and `unbox_str_handle` below the RHS would hand the
/// setter a pre-move `StringHeader*`.
#[test]
fn array_string_key_store_roots_both_operands_across_an_allocating_rhs() {
    assert_operands_rooted_only_when_the_window_collects(
        "array_string_key",
        "js_typed_feedback_array_set_string_key",
        |value| {
            vec![
                Stmt::Let {
                    id: 1,
                    name: "arr".to_string(),
                    ty: Type::Array(Box::new(Type::Any)),
                    mutable: false,
                    init: Some(Expr::Array(vec![Expr::Number(1.0)])),
                },
                Stmt::Let {
                    id: 2,
                    name: "key".to_string(),
                    ty: Type::String,
                    mutable: false,
                    init: Some(Expr::String("k".to_string())),
                },
                Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(2)),
                    value: Box::new(value),
                }),
            ]
        },
    );
}

/// #7639 arm 1 — the polymorphic `o[k] = f()` fallback, reached when nothing
/// about the receiver or the key is statically known. It guarded neither
/// operand, and it is the arm where both are heap values by default.
#[test]
fn polymorphic_index_store_roots_both_operands_across_an_allocating_rhs() {
    assert_operands_rooted_only_when_the_window_collects(
        "polymorphic_index",
        "js_typed_feedback_object_set_index_polymorphic",
        |value| {
            vec![
                Stmt::Let {
                    id: 1,
                    name: "recv".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Object(Vec::new())),
                },
                Stmt::Let {
                    id: 2,
                    name: "key".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Object(Vec::new())),
                },
                Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(2)),
                    value: Box::new(value),
                }),
            ]
        },
    );
}

/// #7640 B tail — declared typed-array dispatch still accepts an arbitrary
/// property key. Evaluating that key may collect before the runtime helper
/// consumes the receiver.
#[test]
fn typed_array_runtime_key_read_roots_receiver_only_when_key_collects() {
    let _native_roots = crate::codegen::helpers::NativeRootsPin::native();
    let compile = |label: &str, key: Expr| {
        compile_body_with_params(
            label,
            vec![param(1, "ta", Type::Named("Int32Array".to_string()))],
            vec![Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(key),
            }))],
        )
    };
    let collecting = compile("ta_runtime_key_collecting", allocating_value());
    let inert = compile("ta_runtime_key_inert", Expr::Undefined);
    let callee = "@js_typed_array_index_get_dynamic(";
    assert!(
        collecting.contains(callee) && inert.contains(callee),
        "both fixtures must reach the typed-array runtime-key arm:\n{collecting}\n{inert}"
    );
    assert_call_operand_rooted_across_operand(
        &collecting,
        "js_typed_array_index_get_dynamic",
        0,
        1,
        "the typed-array receiver",
    );
    assert_eq!(
        root_slots(&collecting),
        root_slots(&inert) + 1,
        "the inert key must add no temporary root, while the collecting key adds exactly \
         the receiver root"
    );
}

/// #7640 A tail — the runtime-key store consumes receiver, key, and value only
/// after all three JavaScript operands have been evaluated.
#[test]
fn typed_array_runtime_key_store_roots_operands_only_when_rhs_collects() {
    let _native_roots = crate::codegen::helpers::NativeRootsPin::native();
    let compile = |label: &str, value: Expr| {
        compile_body_with_params(
            label,
            vec![
                param(1, "ta", Type::Named("Int32Array".to_string())),
                param(2, "key", Type::Any),
            ],
            vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::LocalGet(2)),
                value: Box::new(value),
            })],
        )
    };
    let collecting = compile("ta_runtime_store_collecting", allocating_value());
    let inert = compile("ta_runtime_store_inert", inert_value());
    let callee = "@js_typed_array_index_set_dynamic(";
    assert!(
        collecting.contains(callee) && inert.contains(callee),
        "both fixtures must reach the typed-array runtime-key store arm:\n{collecting}\n{inert}"
    );
    assert_call_operand_rooted_across_operand(
        &collecting,
        "js_typed_array_index_set_dynamic",
        0,
        2,
        "the typed-array receiver",
    );
    assert_call_operand_rooted_across_operand(
        &collecting,
        "js_typed_array_index_set_dynamic",
        1,
        2,
        "the typed-array property key",
    );
    assert_eq!(
        root_slots(&collecting),
        root_slots(&inert) + 2,
        "the inert RHS must add no temporary roots, while the collecting RHS adds exactly \
         the receiver and key roots"
    );
}

/// #7640 A tail — the #5525 inline dynamic typed-array route accepts erased
/// receiver/key types, then lowers a custom-representation RHS before either
/// is consumed by the guard diamond.
#[test]
fn erased_receiver_inline_store_roots_receiver_and_key_across_rhs() {
    let _native_roots = crate::codegen::helpers::NativeRootsPin::native();
    let compile = |label: &str, value: Expr| {
        compile_body_with_params(
            label,
            vec![param(1, "receiver", Type::Any), param(2, "key", Type::Any)],
            vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::LocalGet(2)),
                value: Box::new(value),
            })],
        )
    };
    let collecting = compile("erased_store_collecting", allocating_value());
    let inert = compile("erased_store_inert", inert_value());
    let callee = "@js_dyn_index_set(";
    assert!(
        collecting.contains(callee) && inert.contains(callee),
        "both fixtures must reach the #5525 inline dynamic-store arm:\n{collecting}\n{inert}"
    );
    assert_call_operand_rooted_across_operand(
        &collecting,
        "js_dyn_index_set",
        0,
        2,
        "the erased receiver",
    );
    assert_call_operand_rooted_across_operand(
        &collecting,
        "js_dyn_index_set",
        1,
        2,
        "the erased property key",
    );
    assert_eq!(
        root_slots(&collecting),
        root_slots(&inert) + 2,
        "the inert RHS must add no temporary roots, while the collecting RHS adds exactly \
         the erased receiver and key roots"
    );
}

/// #7640 E follow-up — a cached `BufferViewSlot::data_slot` is safe across a
/// collecting operand only when the construction proves fresh inline storage.
/// View-backed reads/writes must decline before evaluating either operand and
/// let the rooted runtime fallback resolve the current backing pointer.
#[test]
fn collecting_native_view_operands_decline_the_cached_pointer_fast_path() {
    let inert_store = compile_body(
        "native_view_inert_store",
        with_native_f64_view(Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(11)),
            index: Box::new(Expr::Integer(0)),
            value: Box::new(Expr::Number(1.0)),
        })),
    );
    assert!(
        !calls(&inert_store, "js_typed_array_set"),
        "an inert RHS on a proven fixed-length view should retain the inline store:\n{inert_store}"
    );
    assert!(
        inert_store.lines().any(|line| {
            line.trim_start().starts_with("store double 1.0, ptr ") && line.contains("!alias.scope")
        }),
        "the inert fixture must actually emit the native element store, not merely avoid the \
         runtime fallback:\n{inert_store}"
    );

    let collecting_store = compile_body(
        "native_view_collecting_store",
        with_native_f64_view(Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(11)),
            index: Box::new(Expr::Integer(0)),
            value: Box::new(Expr::NumberCoerce(Box::new(allocating_value()))),
        })),
    );
    assert!(
        calls(&collecting_store, "js_typed_array_set"),
        "a collecting RHS must not reuse a native view's cached raw data pointer:\n\
         {collecting_store}"
    );

    let collecting_index = Expr::Binary {
        op: BinaryOp::BitAnd,
        left: Box::new(Expr::NumberCoerce(Box::new(allocating_value()))),
        right: Box::new(Expr::Integer(0)),
    };
    let collecting_load = compile_body(
        "native_view_collecting_load",
        with_native_f64_view(Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(11)),
            index: Box::new(collecting_index),
        }))),
    );
    assert!(
        calls(&collecting_load, "js_typed_array_get"),
        "a collecting proven index must not reuse a native view's cached raw data pointer:\n\
         {collecting_load}"
    );
}

fn compile_masked_window_with_key(
    name: &str,
    key_ty: Type,
    key_mutable: bool,
    mut body: Vec<Stmt>,
) -> String {
    let mut params = vec![param(1, "view", Type::Any)];
    if matches!(key_ty, Type::Int32) {
        // The inert control needs a runtime-derived fact, not an annotation:
        // a generic-ABI Int32 parameter can still receive an object. Keep the
        // standalone-update controls can separately exercise the collector's
        // literal-seeded integer recurrence proof.
        body.insert(
            0,
            Stmt::Let {
                id: 2,
                name: "key".to_string(),
                ty: Type::Any,
                mutable: key_mutable,
                init: Some(Expr::Integer(0)),
            },
        );
    } else {
        params.push(param(2, "key", key_ty));
    }
    compile_body_with_params(name, params, body)
}

fn compile_masked_window_loop(key_ty: Type, value: Expr) -> String {
    compile_masked_window_with_key(
        "masked_window_coercion",
        key_ty,
        false,
        vec![
            Stmt::Let {
                id: 3,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 4,
                    name: "i".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(Expr::Integer(2)),
                }),
                update: Some(Expr::Update {
                    id: 4,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(3, Box::new(value)))],
            },
            Stmt::Return(Some(Expr::LocalGet(3))),
        ],
    )
}

fn masked_window_index_coercion_loop(key_ty: Type) -> String {
    let masked_index = Expr::Binary {
        op: BinaryOp::BitAnd,
        left: Box::new(Expr::Unary {
            op: UnaryOp::Pos,
            operand: Box::new(Expr::LocalGet(2)),
        }),
        right: Box::new(Expr::Integer(7)),
    };
    compile_masked_window_with_key(
        "masked_window_index_coercion",
        key_ty,
        false,
        vec![
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 4,
                    name: "i".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(Expr::Integer(2)),
                }),
                update: Some(Expr::Update {
                    id: 4,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                // A bare read isolates the property under test: whether the
                // unary index coercion is non-collecting.
                body: vec![Stmt::Expr(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(masked_index),
                })],
            },
            Stmt::Return(Some(Expr::Integer(0))),
        ],
    )
}

fn masked_window_rhs_coercion_loop(key_ty: Type) -> String {
    let read = |index| Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Integer(index)),
    };
    compile_masked_window_loop(
        key_ty,
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(read(0)),
                right: Box::new(Expr::Unary {
                    op: UnaryOp::Pos,
                    operand: Box::new(Expr::LocalGet(2)),
                }),
            }),
            right: Box::new(read(1)),
        },
    )
}

fn masked_window_rhs_coercion_region(key_ty: Type) -> String {
    let value = || {
        let read = |index| Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(index)),
        };
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(read(0)),
                right: Box::new(Expr::Unary {
                    op: UnaryOp::Pos,
                    operand: Box::new(Expr::LocalGet(2)),
                }),
            }),
            right: Box::new(read(1)),
        }
    };
    let mut body = vec![Stmt::Let {
        id: 3,
        name: "sum".into(),
        ty: Type::Number,
        init: Some(Expr::Number(0.0)),
        mutable: true,
    }];
    for _ in 0..4 {
        body.push(Stmt::Expr(Expr::LocalSet(3, Box::new(value()))));
    }
    body.push(Stmt::Return(Some(Expr::LocalGet(3))));
    compile_masked_window_with_key("masked_window_rhs_coercion_region", key_ty, false, body)
}

fn masked_window_pair_sum() -> Expr {
    let read = |index| Expr::IndexGet {
        object: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Integer(index)),
    };
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(read(0)),
        right: Box::new(read(1)),
    }
}

fn masked_window_standalone_update_loop(key_ty: Type) -> String {
    compile_masked_window_with_key(
        "masked_window_standalone_update_loop",
        key_ty,
        true,
        vec![
            Stmt::Let {
                id: 3,
                name: "sum".into(),
                ty: Type::Number,
                init: Some(Expr::Number(0.0)),
                mutable: true,
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: 4,
                    name: "i".into(),
                    ty: Type::Any,
                    init: Some(Expr::Integer(0)),
                    mutable: true,
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(4)),
                    right: Box::new(Expr::Integer(2)),
                }),
                update: Some(Expr::Update {
                    id: 4,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(3, Box::new(masked_window_pair_sum()))),
                    Stmt::Expr(Expr::Update {
                        id: 2,
                        op: UpdateOp::Increment,
                        prefix: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(3, Box::new(masked_window_pair_sum()))),
                ],
            },
            Stmt::Return(Some(Expr::LocalGet(3))),
        ],
    )
}

fn masked_window_standalone_update_region(key_ty: Type) -> String {
    let mut body = vec![Stmt::Let {
        id: 3,
        name: "sum".into(),
        ty: Type::Number,
        init: Some(Expr::Number(0.0)),
        mutable: true,
    }];
    body.push(Stmt::Expr(Expr::LocalSet(
        3,
        Box::new(masked_window_pair_sum()),
    )));
    body.push(Stmt::Expr(Expr::Update {
        id: 2,
        op: UpdateOp::Increment,
        prefix: false,
    }));
    for _ in 0..3 {
        body.push(Stmt::Expr(Expr::LocalSet(
            3,
            Box::new(masked_window_pair_sum()),
        )));
    }
    body.push(Stmt::Return(Some(Expr::LocalGet(3))));
    compile_masked_window_with_key("masked_window_standalone_update_region", key_ty, true, body)
}

/// #7640 E review follow-up — unary `+` over an `any` key can invoke user
/// coercion even though the surrounding mask has a static index window. Such
/// an index must decline before a typed-array tier hoists its raw data pointer;
/// the same shape with a literal-seeded integer local proves the fast tier
/// remains live without trusting a parameter annotation.
#[test]
fn collecting_masked_window_index_declines_the_hoisted_pointer_tier() {
    let inert = masked_window_index_coercion_loop(Type::Int32);
    assert!(
        inert.contains("for.packed_f64_range_fast_ta_i32"),
        "the inert control must exercise the masked Int32Array tier:\n{inert}"
    );

    let collecting = masked_window_index_coercion_loop(Type::Any);
    assert!(
        calls(&collecting, "js_number_coerce"),
        "unary + over an any key must exercise the collecting coercion witness:\n{collecting}"
    );
    assert!(
        !collecting.contains("for.packed_f64_range_fast_ta_i32"),
        "a collecting masked index must decline before the tier hoists a raw backing pointer:\n\
         {collecting}"
    );
}

/// The same proof must cover coercion BETWEEN masked reads, not only inside an
/// index. Otherwise the second read consumes the tier's hoisted pointer after
/// `+key` has been allowed to run user code and move or dispose its backing.
#[test]
fn collecting_rhs_between_masked_reads_declines_the_hoisted_pointer_tier() {
    let inert = masked_window_rhs_coercion_loop(Type::Int32);
    assert!(
        inert.contains("for.packed_f64_range_fast_ta_i32"),
        "the inert RHS control must retain the masked Int32Array tier:\n{inert}"
    );

    let collecting = masked_window_rhs_coercion_loop(Type::Any);
    assert!(
        calls(&collecting, "js_number_coerce"),
        "the any-typed RHS must exercise the user-coercion witness:\n{collecting}"
    );
    assert!(
        !collecting.contains("for.packed_f64_range_fast_ta_i32"),
        "collecting coercion between masked reads must decline the hoisted-pointer tier:\n\
         {collecting}"
    );
}

/// The straight-line masked region installs the same pointer facts, including
/// for later store operands. Exercise that caller independently of the loop
/// matcher so the shared whole-expression gate cannot regress on either path.
#[test]
fn collecting_rhs_declines_the_straight_line_masked_region() {
    let inert = masked_window_rhs_coercion_region(Type::Int32);
    assert!(
        inert.contains("masked_region.ta_i32.preheader"),
        "the inert RHS control must retain straight-line masked versioning:\n{inert}"
    );

    let collecting = masked_window_rhs_coercion_region(Type::Any);
    assert!(
        calls(&collecting, "js_number_coerce"),
        "the any-typed region RHS must exercise the user-coercion witness:\n{collecting}"
    );
    assert!(
        !collecting.contains("masked_region.ta_i32.preheader"),
        "collecting coercion must decline the straight-line masked region:\n{collecting}"
    );
}

/// A standalone update is not part of an index/RHS tree, but it executes in
/// the same hoisted-pointer copy between masked reads. `any++` can run user
/// ToNumeric hooks; a literal-seeded integer recurrence remains call-free.
#[test]
fn collecting_standalone_update_declines_the_masked_loop() {
    let inert = masked_window_standalone_update_loop(Type::Int32);
    assert!(
        inert.contains("for.packed_f64_range_fast_ta_i32"),
        "an inert standalone update must retain the masked loop tier:\n{inert}"
    );

    let collecting = masked_window_standalone_update_loop(Type::Any);
    assert!(
        calls(&collecting, "js_to_numeric"),
        "an any-typed standalone update must exercise collecting ToNumeric:\n{collecting}"
    );
    assert!(
        !collecting.contains("for.packed_f64_range_fast_ta_i32"),
        "collecting standalone update must decline the hoisted-pointer loop tier:\n{collecting}"
    );
}

/// Straight-line region matching has its own standalone-Update arm and must
/// apply the identical inert-target gate before spanning later masked reads.
#[test]
fn collecting_standalone_update_declines_the_masked_region() {
    let inert = masked_window_standalone_update_region(Type::Int32);
    assert!(
        inert.contains("masked_region.ta_i32.preheader"),
        "an inert standalone update must retain straight-line versioning:\n{inert}"
    );

    let collecting = masked_window_standalone_update_region(Type::Any);
    assert!(
        calls(&collecting, "js_to_numeric"),
        "an any-typed region update must exercise collecting ToNumeric:\n{collecting}"
    );
    assert!(
        !collecting.contains("masked_region.ta_i32.preheader"),
        "collecting standalone update must decline the straight-line masked region:\n{collecting}"
    );
}

/// #7640 E — the array-grow helper may return a replacement allocation. The
/// write barrier on that path must therefore shade through the returned head,
/// not the raw receiver handle computed before the call.
#[test]
fn growing_array_store_uses_the_reallocated_head_for_its_barrier() {
    let ir = compile_body(
        "array_grow_barrier_head",
        vec![
            Stmt::Let {
                id: 1,
                name: "arr".to_string(),
                ty: Type::Array(Box::new(Type::Any)),
                mutable: false,
                init: Some(Expr::Array(vec![Expr::Undefined])),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(8)),
                value: Box::new(allocating_value()),
            }),
        ],
    );
    let realloc_call = ir
        .lines()
        .find(|line| line.contains(" = call i64 @js_array_set_f64_extend"))
        .unwrap_or_else(|| panic!("fixture never emitted the realloc path:\n{ir}"));
    let new_head = realloc_call
        .split_once(" = ")
        .map(|(result, _)| result.trim())
        .expect("realloc call has an SSA result");
    let realloc_label = ir
        .lines()
        .position(|line| line.starts_with("idxset.realloc."))
        .unwrap_or_else(|| panic!("fixture never emitted an idxset.realloc block:\n{ir}"));
    let realloc_body = ir
        .lines()
        .skip(realloc_label + 1)
        .take_while(|line| line.starts_with(char::is_whitespace) || line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let barrier = realloc_body
        .lines()
        .find(|line| line.contains("@js_write_barrier_slot("))
        .unwrap_or_else(|| panic!("realloc path lost its write barrier:\n{realloc_body}"));
    assert!(
        barrier.contains(&format!("i64 {new_head}")),
        "the realloc-path barrier must use {new_head}, returned by the grow helper; got `{barrier}`"
    );
}
