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
//! a later operand is either allocating or inert. Then it compares the
//! shadow-frame width the function reserves. The realloc-barrier regression at
//! the end instead names the exact SSA head consumed by its barrier.
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
use perry_hir::{Expr, Function, Module as HirModule, Param, Stmt};

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

/// An allocating RHS (`{ a: 1 }`) and an inert one (`1`), so each test can
/// compare the same store under a collecting and a non-collecting window.
fn allocating_value() -> Expr {
    Expr::Object(vec![("a".to_string(), Expr::Number(1.0))])
}

fn inert_value() -> Expr {
    Expr::Number(1.0)
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
    assert!(
        root_slots(&collecting) > root_slots(&inert),
        "an allocating runtime key must protect the typed-array receiver"
    );
}

/// #7640 A tail — the runtime-key store consumes receiver, key, and value only
/// after all three JavaScript operands have been evaluated.
#[test]
fn typed_array_runtime_key_store_roots_operands_only_when_rhs_collects() {
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
    assert!(
        root_slots(&collecting) > root_slots(&inert),
        "an allocating RHS must protect the typed-array receiver and key"
    );
}

/// #7640 A tail — the #5525 inline dynamic typed-array route accepts erased
/// receiver/key types, then lowers a custom-representation RHS before either
/// is consumed by the guard diamond.
#[test]
fn erased_receiver_inline_store_roots_receiver_and_key_across_rhs() {
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
    let extra_slots = root_slots(&collecting).saturating_sub(root_slots(&inert));
    assert!(
        extra_slots >= 2,
        "an allocating RHS must protect both erased operands; expected at least two extra slots, got {extra_slots}"
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
