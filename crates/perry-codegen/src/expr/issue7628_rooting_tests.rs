//! Rooting coverage for `a[i]++` / `o.f++` (#7628).
//!
//! # What the issue claimed, and what the A/B actually showed
//!
//! Both lowerings are a read-modify-write whose two operands are consumed by
//! **four** calls with collection points between them:
//!
//! ```text
//! old     = js_dyn_index_get(obj, idx)          ; a getter / Proxy trap
//! old_num = js_to_numeric(old)                  ; a valueOf
//! new     = js_numeric_step(old_num, step)
//!           js_put_value_set(obj, idx, new, obj, strict) ; setter / Proxy trap
//! ```
//!
//! #7628 filed the operand pair as a live #7154: `with_operands_rooted`
//! re-reads at exactly ONE point, so the registers `js_put_value_set` reads
//! were the ones produced above `js_dyn_index_get`. **On the emitted IR that is
//! not what happens, and the sabotage arm is how that was found rather than
//! argued.** Collapsing the per-use re-reads back to one — and, for
//! `PropertyUpdate`, dropping the receiver's root entirely — leaves the emitted
//! IR *unchanged in the relevant respect*: `root_reload` (#7280) rematerialises
//! the slot load at each use a collection point can reach:
//!
//! ```llvm
//!   %r49.rs4p = load ptr addrspace(1), ptr %r29     ; inserted by root_reload
//!   %r49      = ptrtoint ptr addrspace(1) %r49.rs4p to i64
//!   %recv     = bitcast i64 %r49 to double
//!   call double @js_put_value_set(double %recv, double %key, double %new,
//!                                 double %recv, i32 %strict)
//! ```
//!
//! So the operand half of #7628 is **not a live bug on the default build**, and
//! the per-use re-reads the source now emits are belt-and-braces: they cost
//! nothing (the pass would emit them anyway) and they stop the arm depending on
//! a pass that carries a documented side condition ("unless a store to that
//! slot can also run on the way") and a corpus allowlist.
//!
//! # What IS a live bug, and the test that discriminates it
//!
//! The RESULT. `js_to_numeric` / `js_numeric_step` hand back a heap
//! `BigIntHeader` for a BigInt element, and whichever of the two the expression
//! yields — `old_num` for postfix, `new` for prefix — is live across
//! `js_put_value_set`, i.e. across a user setter. It is a bare call result with
//! **no slot**, so `root_reload` has nothing to reload from; that is the
//! taxonomy's case (d) and the one this fix closes with
//! `RootedGroup::adopt_emitted`.
//!
//! [`the_result_is_rooted_only_when_the_element_may_be_a_bigint`] is the only
//! test here with a counterfactual, and it carries its own: the typed-array arm
//! (`is_provably_not_bigint` proves the element is `Number | undefined`) takes
//! `protect == false` and its returned register is produced ABOVE the write —
//! measured, not assumed. That is the same shape as the unrooted lowering, so
//! the two arms together show the root is what moves the read.
//!
//! # The remaining test is a PIPELINE assertion, and says so
//!
//! [`the_emitted_ir_rereads_both_operands_below_the_read`] holds because of
//! `root_reload`, not because of this file's source form, and it cannot fail on
//! a change to `expr/instance_misc1.rs` alone. It is kept anyway because it can
//! fail on a `root_reload` regression for this shape, which is a property
//! nothing else pins. It is NOT evidence about the lowering, and naming it
//! otherwise is how a green gate stops meaning anything.
//!
//! Slot counts are avoided throughout for the reason slice 8 recorded: under
//! statepoints `reserve_shadow_slot` returns a stack-map index and no
//! `js_shadow_frame_enter` is emitted at all, so a frame-width assertion reads
//! zero and passes on the default build. `temp_root_push_double` is likewise a
//! plain alloca `store` in alloca mode, so counting `js_gc_temp_root_push` reads
//! zero here too — the first version of the third test did exactly that and
//! compared 0 against 0.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module as HirModule, Stmt};

use super::slice7_rooting_tests::require_call_line;
use super::slice8_rooting_tests::{call_operand_of, producer_line};

fn compile_body(name: &str, body: Vec<Stmt>) -> String {
    let mut hir = HirModule::new(name);
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
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

/// Assert that operand `n` of `writer` is produced strictly below the call to
/// `reader` — i.e. it was re-read after the window rather than carried across
/// it.
fn assert_operand_reread_below(ir: &str, writer: &str, n: usize, reader: &str, what: &str) {
    let window = require_call_line(ir, reader);
    let reg = call_operand_of(ir, writer, n);
    let produced = producer_line(ir, &reg);
    assert!(
        produced > window,
        "{what}: {writer} reads {reg} as operand {n}, produced at line {produced} — at or \
         ABOVE line {window}, where {reader} runs. {reader} can re-enter user code, so an \
         evacuating cycle inside it relocates the object and that register names from-space. \
         It has to be re-read below the call, not carried across it.\n{ir}"
    );
}

/// `const o = { items: [1, 2] };` — the receiver is reached through a FIELD
/// read rather than a local, for the reason slice 8's header records: for a
/// local with a shadow slot the `ptr addrspace(1)` retype pass rematerialises
/// the load at the use site, so an unrooted lowering can read fresh *by
/// accident* and the test goes green against the bug.
fn with_object_local(tail: Stmt) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: 0,
            name: "o".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Object(vec![(
                "items".to_string(),
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)]),
            )])),
        },
        tail,
    ]
}

fn field_of_o(property: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(0)),
        property: property.to_string(),
        byte_offset: 0,
    }
}

fn items_of_o() -> Expr {
    field_of_o("items")
}

/// The register the function returns.
///
/// Used instead of a slot count on purpose: `temp_root_push_double` lowers to a
/// `js_gc_temp_root_push` CALL only in the FFI-fallback mode. In alloca mode it
/// is a plain `store` into a pooled alloca and under statepoints
/// `reserve_shadow_slot` hands back a stack-map index — so counting the helper
/// name reads zero on the default build and passes vacuously, which is exactly
/// the trap slice 8's header records.
fn returned_register(ir: &str) -> String {
    ir.lines()
        .find(|l| l.trim_start().starts_with("ret double %"))
        .unwrap_or_else(|| panic!("no `ret double %<reg>` in:\n{ir}"))
        .trim()
        .rsplit(' ')
        .next()
        .expect("ret has an operand")
        .to_string()
}

/// `o.items[o.n]++` — the emitted IR re-reads both operands below
/// `js_dyn_index_get`.
///
/// ★ **A pipeline assertion, not a statement about this lowering.** It holds
/// via `root_reload` and stays green with the source-level re-reads collapsed
/// (measured — see the module header), so it cannot fail on a change to
/// `expr/instance_misc1.rs` alone. What it can catch is a `root_reload`
/// regression for this shape.
///
/// The index is a field read rather than a literal because a literal is a
/// constant with no definition to order against (`Reuse`, correctly).
#[test]
fn the_emitted_ir_rereads_both_operands_below_the_read() {
    let ir = compile_body(
        "issue7628_index_update",
        with_object_local(Stmt::Expr(Expr::IndexUpdate {
            object: Box::new(items_of_o()),
            index: Box::new(field_of_o("n")),
            op: BinaryOp::Add,
            prefix: false,
            strict: true,
        })),
    );
    // The generic arm was reached — without this the rest is vacuous.
    require_call_line(&ir, "js_dyn_index_get");
    assert_operand_reread_below(
        &ir,
        "js_put_value_set",
        0,
        "js_dyn_index_get",
        "the IndexUpdate receiver",
    );
    assert_operand_reread_below(
        &ir,
        "js_put_value_set",
        1,
        "js_dyn_index_get",
        "the IndexUpdate index",
    );
}

/// `o.count++` — the emitted IR re-derives the RAW `i64` receiver and key
/// handles below `js_object_get_field_by_name_f64`.
///
/// ★ Also a pipeline assertion. #7280's taxonomy lists "a pointer already
/// unboxed to raw `i64`" as case (a), the class `root_reload` cannot repair —
/// but that is about a raw handle a helper RETURNS, not one masked out of a
/// NaN-boxed value the pass has spilled. Here the chain's root is a
/// `load ptr addrspace(1)`, and the pass rematerialises the whole
/// `load` → `ptrtoint` → `and` chain at the use. Verified by dropping the
/// receiver's root entirely: still green.
#[test]
fn the_emitted_ir_rederives_the_raw_handles_below_the_read() {
    let ir = compile_body(
        "issue7628_property_update",
        with_object_local(Stmt::Expr(Expr::PropertyUpdate {
            object: Box::new(items_of_o()),
            property: "count".to_string(),
            op: BinaryOp::Add,
            prefix: false,
            strict: true,
        })),
    );
    require_call_line(&ir, "js_object_get_field_by_name_f64");
    assert_operand_reread_below(
        &ir,
        "js_put_value_set",
        0,
        "js_object_get_field_by_name_f64",
        "the PropertyUpdate receiver",
    );
    assert_operand_reread_below(
        &ir,
        "js_put_value_set",
        1,
        "js_object_get_field_by_name_f64",
        "the PropertyUpdate key",
    );
}

/// The RESULT of `a[i]++` is live across `js_put_value_set`, which runs a user
/// setter — so for an element that may be a BigInt it must be re-read below
/// that call.
///
/// And the zero-cost half, which is the same assertion inverted: a typed-array
/// element is `Number | undefined` by construction, so
/// `is_provably_not_bigint` proves neither `js_to_numeric`'s result nor
/// `js_numeric_step`'s can be a heap `BigIntHeader`, `protect` is `false`, and
/// the arm keeps the register it had. Without the second half a future "root
/// every result" widening would tax every update unnoticed.
#[test]
fn the_result_is_rooted_only_when_the_element_may_be_a_bigint() {
    let unproven = compile_body(
        "issue7628_result_root",
        with_object_local(Stmt::Return(Some(Expr::IndexUpdate {
            object: Box::new(items_of_o()),
            index: Box::new(field_of_o("n")),
            op: BinaryOp::Add,
            prefix: false,
            strict: true,
        }))),
    );
    let write = require_call_line(&unproven, "js_put_value_set");
    let produced = producer_line(&unproven, &returned_register(&unproven));
    assert!(
        produced > write,
        "a BigInt-capable element's postfix result is live across js_put_value_set (a user \
         setter) and must be re-read below it — produced at line {produced}, the write is at \
         line {write}.\n{unproven}"
    );

    let typed = compile_body(
        "issue7628_result_no_root",
        vec![
            Stmt::Let {
                id: 0,
                name: "ta".to_string(),
                ty: Type::Named("Uint8Array".to_string()),
                mutable: false,
                init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(4.0))))),
            },
            Stmt::Return(Some(Expr::IndexUpdate {
                object: Box::new(Expr::LocalGet(0)),
                index: Box::new(Expr::Number(0.0)),
                op: BinaryOp::Add,
                prefix: false,
                strict: true,
            })),
        ],
    );
    let typed_write = require_call_line(&typed, "js_put_value_set");
    let typed_produced = producer_line(&typed, &returned_register(&typed));
    assert!(
        typed_produced < typed_write,
        "a typed-array element can never be a BigInt, so the result must keep the register \
         js_to_numeric produced — no slot, no re-read. Produced at line {typed_produced}, the \
         write is at line {typed_write}; if this is BELOW the write the \
         `is_provably_not_bigint` gate has stopped gating.\n{typed}"
    );
}
