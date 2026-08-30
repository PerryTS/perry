//! Issue #1098: extracted shadow-slot helper free functions.
//!
//! Pure mechanical move out of `expr/mod.rs`. These `pub(crate)` free
//! functions are re-exported from the trunk so existing
//! `crate::expr::X` call paths resolve unchanged.
use super::*;

use anyhow::{anyhow, Result};

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use crate::types::{I32, I64, PTR};

/// The current closure pointer to feed to `js_closure_get/set_capture_bits`,
/// re-read from the shadow-rooted slot when one exists (#7055).
///
/// Every capture access MUST go through here rather than reusing the
/// `%this_closure` SSA parameter. The parameter is a register value the
/// collector cannot see: an evacuating young collection at a loop back-edge
/// poll inside the body relocates the closure, rewrites every root it knows
/// about, and then resets from-space — after which the register points at
/// recycled memory whose `capture_count` no longer covers the index, so
/// `js_closure_get_capture_bits` returns 0 and every subsequent boxed-capture
/// read/write silently no-ops. Returns `None` when the body has no closure
/// pointer at all (top-level functions/methods).
pub(crate) fn try_current_closure_ptr_value(ctx: &mut FnCtx<'_>) -> Option<String> {
    if let Some(slot) = ctx.current_closure_slot.clone() {
        let bits = ctx.block().load(I64, &slot);
        return Some(ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64));
    }
    ctx.current_closure_ptr.clone()
}

/// [`try_current_closure_ptr_value`] with the shared "no current_closure_ptr"
/// diagnostic; `what` names the lowering that needed it.
pub(crate) fn current_closure_ptr_value(ctx: &mut FnCtx<'_>, what: &str) -> Result<String> {
    try_current_closure_ptr_value(ctx).ok_or_else(|| anyhow!("{what} but no current_closure_ptr"))
}

pub(crate) fn expr_is_known_non_pointer_shadow_value(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) => true,
        Expr::LocalGet(id) => {
            // Whole-function write analysis proves these locals numeric by
            // construction.  That proof does not depend on a TypeScript
            // annotation and remains valid at every read, including loop
            // counters whose back-edge update makes an initializer-only
            // proof ineligible.
            if ctx.integer_locals.contains(id) || ctx.number_by_construction_locals.contains(id) {
                return true;
            }
            // A reserved shadow slot means the local is pointer-possible even
            // if its initializer refined `local_types` to a scalar.
            //
            // #7236: this list also carried `HirType::Symbol`, and it is a
            // SEPARATE hazard from the missing shadow slot — a `true` here
            // suppresses `temp_root`'s argument/operand rooting, so
            // `obj[s] = alloc()` with `s: symbol` left the movable symbol
            // unrooted across the allocating call even once the local itself
            // had a slot. `lower_call/closure_analysis.rs`'s
            // `local_is_inert_primitive` never listed `Symbol`; this copy and
            // `collectors/pointer_locals.rs` were the two that did.
            //
            // Derived from the one definition rather than restated, but kept
            // NON-`Union` on purpose: `type_is_pointer_bearing` answers `false`
            // for an all-scalar union (`number | undefined`), which would
            // WIDEN this suppression to locals it never covered. That is a
            // plausible optimisation and an unmeasured one; it is not this
            // change. The guard makes the arm exactly the old list minus
            // `Symbol`.
            !ctx.shadow_slot_map.contains_key(id)
                && ctx.stable_local_type_proof(id).is_some_and(|ty| {
                    !matches!(ty, HirType::Union(_))
                        && !crate::typed_shape::type_is_pointer_bearing(ty)
                })
        }
        Expr::Compare { .. } | Expr::Void(_) => true,
        Expr::Unary { op, operand } => match op {
            UnaryOp::Not | UnaryOp::Pos => true,
            UnaryOp::Neg | UnaryOp::BitNot => {
                crate::type_analysis::is_provably_not_bigint(ctx, operand)
            }
        },
        // `+` is the only BinaryOp that can produce a string, so it is the
        // only one whose result needs its operands proven numeric. Every
        // other operator applies ToNumeric to both sides and yields a Number
        // or a BigInt whatever they were — so once BigInt is excluded the
        // result cannot be a pointer, and an accumulator written by `-=`,
        // `*=`, `^=` and friends stops paying a write barrier per store for a
        // case its operator cannot reach.
        Expr::Binary { op, .. } if !matches!(op, BinaryOp::Add) => {
            crate::type_analysis::is_provably_not_bigint(ctx, expr)
        }
        Expr::Binary { .. } => {
            crate::type_analysis::is_numeric_expr(ctx, expr)
                && crate::type_analysis::is_provably_not_bigint(ctx, expr)
        }
        // #6750 follow-up: a masked-index read covered by an ACTIVE
        // masked-window fact is a guard-proven numeric element load — never
        // a pointer — even when the receiver's static type is erased.
        Expr::IndexGet { object, index } => {
            matches!(
                object.as_ref(),
                Expr::LocalGet(arr_id)
                    if super::masked_window_fact_for_index(ctx, *arr_id, index).is_some()
            ) || super::is_proven_u32_view_read(ctx, expr)
        }
        // #6996: a typed-array / Buffer element read is a number (or
        // `undefined` out of range) BY CONSTRUCTION -- `lower_buffer_load`'s
        // inline byte load, `js_uint8array_index_get_value` and
        // `js_buffer_index_get_value` can only ever produce one. It is never a
        // heap reference, so #6951's argument-temporary rooting has nothing to
        // protect and its push/re-read/truncate trio is pure TLS traffic in
        // exactly the loops that can least afford it (`buf[i] + packet.tag`
        // rooted the byte across the property get, once per iteration).
        //
        // The proof is about the LOWERING, not the declared type: annotations
        // are unenforced, so `buf: Buffer` holding something else must not be
        // load-bearing -- and it isn't, because both runtime accessors answer
        // `undefined` for a receiver that is not a Uint8Array/Buffer.
        //
        // The three lowerings of this node that CAN yield a heap value are
        // excluded by construction, one condition each:
        //   * a symbol key (`u8[Symbol.iterator]`) goes to
        //     `js_object_get_symbol_property`, which returns the accessor;
        //   * in JS-value context, a key without the integer-array-index proof
        //     goes to `js_typed_array_index_get_dynamic`, which falls through
        //     to string-keyed property lookup (an expando holds anything);
        //   * in i32 context (`lower_uint8array_get_i32`), a key that is not
        //     numeric-proven goes to `js_object_get_index_polymorphic`, which
        //     dispatches a string key to that same property path. That arm is
        //     gated on `is_numeric_expr`, NOT on the index proof, so testing
        //     the proof alone would not cover it -- both conditions are
        //     required. `is_numeric_expr` is used here only to NARROW: a wrong
        //     `true` from it still leaves a byte read on every arm it admits.
        // `BufferIndexGet` has none of these paths -- every arm coerces the key
        // to i32 and reads a byte -- so it needs no condition.
        Expr::Uint8ArrayGet { index, .. } => {
            !matches!(index.as_ref(), Expr::SymbolFor(_))
                && crate::type_analysis::is_numeric_expr(ctx, index)
                && super::index_get::numeric_index_has_integer_array_index_proof(ctx, index)
        }
        Expr::BufferIndexGet { .. } => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_is_known_non_pointer_shadow_value(ctx, then_expr)
                && expr_is_known_non_pointer_shadow_value(ctx, else_expr)
        }
        Expr::Sequence(exprs) => exprs
            .last()
            .is_some_and(|last| expr_is_known_non_pointer_shadow_value(ctx, last)),
        _ => false,
    }
}

pub(crate) fn emit_shadow_slot_clear(ctx: &mut FnCtx<'_>, slot_idx: u32) {
    if ctx.persistent_shadow_slots.contains(&slot_idx) {
        return;
    }
    // #7771: inside an element-shape fast clone the tracked `const r = arr[j]`
    // binding is VIRTUAL — its `Let` emits nothing (`stmt/let_stmt.rs`) and
    // its slot is never (re)bound in the clone, so this lexical-death clear
    // would be the clone's ONLY runtime call. Depending on the shadow-frame
    // mode that call is real (`js_shadow_slot_set`), and a call inside a
    // call-free-by-construction clone does not slow it, it DELETES it
    // (#7690). Skipping is sound in every mode: the slot still holds whatever
    // it held before the loop, a stale-but-rooted value is over-rooting that
    // a moving collection rewrites like any root, and every later user of a
    // shared slot index binds before use. The slow clone, lowered after the
    // fact is popped, keeps its clear.
    if ctx.element_shape_loop_facts.iter().any(|fact| {
        fact.element_binding
            .and_then(|id| ctx.shadow_slot_map.get(&id))
            == Some(&slot_idx)
    }) {
        return;
    }
    // Never-bound slot: it provably still holds its initial 0 (slots are only
    // written through bind/set, and every value-set site binds first), so the
    // clear would be a redundant `js_shadow_slot_set(idx, 0)` TLS hit.
    if !ctx.shadow_slots_bound.contains(&slot_idx) {
        return;
    }
    // #6794 follow-up (b): the slot was already cleared to 0 for a currently
    // shadow-suppressed masked-window-region local, and suppression blocks every
    // subsequent write to it (`emit_shadow_slot_update_for_expr`), so it provably
    // still holds 0 — this clear is a redundant `js_shadow_slot_set(slot, 0)`
    // (the `_tlv_get_addr` TLS hit that dominated bcryptjs `_encipher`). Skip it.
    if ctx.suppressed_cleared_shadow_slots.contains(&slot_idx) {
        return;
    }
    // #7088: emitted inline against this activation's cached `ShadowStackState`
    // pointer when it has one; falls through to the call otherwise.
    if super::shadow_inline::emit_inline_slot_clear(ctx, slot_idx) {
        return;
    }
    ctx.block().call_void(
        "js_shadow_slot_set",
        &[(I32, &slot_idx.to_string()), (I64, "0")],
    );
}

/// Bind an immutable `const item = rootedArray[index]` local once in the
/// function-entry setup and retain its current value until return.
///
/// The alloca is entry-hoisted and initialized to `undefined`, so the early
/// bind is valid even when the declaration itself sits in a loop or branch.
/// Every later iteration writes the same alloca, which the GC scanner follows
/// through `slot_ptrs`. Pointer-capable updates still emit the root shading
/// barrier required when an incremental collection has already scanned roots;
/// only the repeated TLS slot rebinding and lexical-death clear are removed.
pub(crate) fn enable_persistent_shadow_slot_for_array_alias(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    init: &Expr,
) {
    let Expr::IndexGet { object, .. } = init else {
        return;
    };
    if !matches!(object.as_ref(), Expr::LocalGet(_)) {
        return;
    }
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    let Some(local_slot) = ctx.locals.get(&local_id).cloned() else {
        return;
    };
    if !ctx.persistent_shadow_slots.insert(slot_idx) {
        return;
    }
    ctx.shadow_slots_bound.insert(slot_idx);
    let slot_idx_string = slot_idx.to_string();
    ctx.func.entry_setup_call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx_string), (PTR, &local_slot)],
    );
}

pub(crate) fn emit_shadow_slot_bind_for_local(ctx: &mut FnCtx<'_>, local_id: u32) {
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    if ctx.persistent_shadow_slots.contains(&slot_idx) {
        return;
    }
    // #8132: a boxed local's alloca never holds a GC-heap value, so rooting it
    // protects nothing and (under the RS4GC lowering) costs a relocation of
    // the box pointer at EVERY statepoint it is live across. Every store site
    // routes through the same `boxed_vars && !module_globals` test
    // (`stmt/mod.rs` prealloc, `let_stmt.rs`'s boxed arm,
    // `codegen/arguments.rs::store_param_slot`, `lower_call/new_ctor_args.rs`),
    // and each of them stores only a `js_box_alloc_bits`-family result or the
    // TAG_UNDEFINED sentinel into the slot — the VALUE always goes inside the
    // box. Boxes are `std::alloc` allocations outside the GC heap: no
    // collector phase moves them, box.rs never frees them (`BOX_REGISTRY` is
    // monotonic), and the JSValue inside is traced and rewritten by the
    // registered `scan_box_roots_mut` scanner. All three premises are pinned
    // by `scripts/gc_root_dominance_check.py`'s IMMOVABLE_SOURCES "box" entry,
    // whose probes fail the lint if boxes ever become arena-allocated or grow
    // a free path — at which point this skip must be reverted with them.
    //
    // On the webpack-factory monolith of #8132, ~300 preallocated boxes were
    // live across ~90% of one function's 5.5k statepoints; unbinding them is
    // what "not modelling every value as a GC pointer where a proof exists"
    // means for this shape.
    if ctx.boxed_vars.contains(&local_id) && !ctx.module_globals.contains_key(&local_id) {
        return;
    }
    let Some(local_slot) = ctx.locals.get(&local_id).cloned() else {
        return;
    };
    emit_shadow_slot_bind_ptr(ctx, slot_idx, &local_slot);
}

/// Bind frame slot `slot_idx` to the root alloca `slot_ptr` — the raw form of
/// [`emit_shadow_slot_bind_for_local`], for roots that are not named locals
/// (#7469: the pooled temp-root allocas in `rooting/temp_root.rs`).
///
/// The caller owns the pairing of `slot_idx` and `slot_ptr`; everything else
/// — the stack-map textual marker, the #7088 inline frame write, the FFI
/// fallback, and the incremental root-shading barrier — is identical to a
/// named local's bind, which is the point: a temp rooted through here is
/// indistinguishable to the collector and to the RS4GC/stack-map lowering
/// from a local.
pub(crate) fn emit_shadow_slot_bind_ptr(ctx: &mut FnCtx<'_>, slot_idx: u32, slot_ptr: &str) {
    ctx.shadow_slots_bound.insert(slot_idx);
    if crate::codegen::helpers::native_stack_roots_enabled() {
        // Kept temporarily as a textual marker: LlFunction's final stack-map
        // lowering records `slot_idx -> slot_ptr` and removes this call.
        // The incremental root barrier remains real because the native slot
        // can be updated after an in-flight cycle scanned this frame.
        ctx.block().call_void(
            "js_shadow_slot_bind",
            &[(I32, &slot_idx.to_string()), (PTR, slot_ptr)],
        );
        let value_bits = ctx.block().load(I64, slot_ptr);
        emit_persistent_shadow_root_barrier(ctx, &value_bits);
        return;
    }
    // #7088: the hot per-store root write. Emitted inline against this
    // activation's cached `ShadowStackState` pointer when it has one; falls
    // through to the call otherwise.
    if super::shadow_inline::emit_inline_slot_bind(ctx, slot_idx, slot_ptr) {
        return;
    }
    ctx.block().call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx.to_string()), (PTR, slot_ptr)],
    );
}

/// Emit the incremental-mark root shading barrier for a value that has just
/// been written into an already-bound (persistent) root slot.
///
/// This is the only part of `js_shadow_slot_bind` that is genuinely per-store:
/// re-recording `slot_ptrs[idx]` and re-mirroring the value are loop-invariant
/// for an entry-hoisted alloca, but a pointer stored into a root *after* the
/// collector scanned roots still has to be shaded. Guarding on
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` inline keeps the common
/// (no incremental cycle in flight) path down to a load, a compare, and a
/// not-taken branch instead of a TLS-touching call. The load is LLVM
/// `monotonic`, matching the runtime's Rust `Relaxed` readers: the counter is
/// only a gate and does not publish accompanying memory.
pub(crate) fn emit_persistent_shadow_root_barrier(ctx: &mut FnCtx<'_>, value_bits: &str) {
    // #8583-followup: if computing the value diverged (a throwing sub-expression
    // — e.g. a TDZ access on a captured `let` — emitted `unreachable`), the
    // current block is terminated. `LlBlock` drops instructions emitted after a
    // terminator, so `value_bits`' defining instruction was silently discarded;
    // the barrier block created below would then reference an undefined register
    // ("register %rN used but never defined"). The root store is unreachable on
    // this path, so emit no barrier.
    if ctx.block().is_terminated() {
        return;
    }
    let active =
        ctx.block()
            .load_atomic_monotonic(I32, "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT", 4);
    let barrier_needed = ctx.block().icmp_ne(I32, &active, "0");
    let barrier_idx = ctx.new_block("shadow.root.barrier");
    let done_idx = ctx.new_block("shadow.root.barrier.done");
    let barrier_label = ctx.block_label(barrier_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block()
        .cond_br(&barrier_needed, &barrier_label, &done_label);

    ctx.current_block = barrier_idx;
    ctx.block()
        .call_void("js_write_barrier_root_nanbox", &[(I64, value_bits)]);
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

/// #9081: root the pointer locals of a constructor body spliced inline into
/// the CURRENT function — the `super(...)` parent-body inline, the `new`-site
/// own/inherited-ctor inline, and `let_stmt`'s scalar-ctor variants.
///
/// The enclosing function's `shadow_slot_map` was computed by
/// `collect_pointer_typed_locals` over that function's OWN params and body;
/// an inlined constructor body's locals are invisible to it. Its `Let`s then
/// land in plain entry allocas that are neither shadow slots nor temp roots,
/// so a moving minor during the body leaves them holding the pre-move
/// address (three.js: `RenderTarget`'s `const texture = new Texture(...)`
/// inlined into `WebGLRenderTarget_constructor`, stale by `texture.clone()`).
///
/// Runs the same collector over the spliced params+body and extends the map
/// through `reserve_shadow_slot`, which grows the already-emitted frame in
/// place on both root backends (stack-map count and shadow-frame push).
/// `Let`/assignment sites then mirror stores through the ordinary bind path.
/// An id already in `ctx.locals` (a ctor param bound by the caller before
/// this runs) is bound here — unconditionally, because a second inline of
/// the same constructor in this function binds fresh param allocas that must
/// re-point the existing slot. Local ids are module-unique, so extending the
/// map never aliases an enclosing local.
pub(crate) fn root_inlined_ctor_pointer_locals(
    ctx: &mut FnCtx<'_>,
    params: &[perry_hir::Param],
    body: &[perry_hir::Stmt],
) {
    if !crate::codegen::helpers::precise_root_analysis_enabled() {
        return;
    }
    let flat_const_ids: std::collections::HashSet<u32> =
        ctx.flat_const_arrays.keys().copied().collect();
    let pointer_locals =
        crate::collectors::collect_pointer_typed_locals(params, body, &flat_const_ids);
    // Slot indices must not depend on HashMap iteration order.
    let mut ids: Vec<u32> = pointer_locals.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        if !ctx.shadow_slot_map.contains_key(&id) {
            let Some(slot_idx) = ctx.func.reserve_shadow_slot() else {
                return;
            };
            ctx.shadow_slot_map.insert(id, slot_idx);
        }
        if ctx.locals.contains_key(&id) {
            emit_shadow_slot_bind_for_local(ctx, id);
        }
    }
}

pub(crate) fn emit_shadow_slot_update_for_expr(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    value_reg: &str,
    rhs: &Expr,
) {
    // #6750 follow-up: inside a masked-window region fast copy, a local
    // flow-refined to Number had its slot cleared at the refinement point
    // and every subsequent region write stores a proven number — no
    // per-statement shadow traffic needed until the refinement is dropped
    // (see `stmt::masked_window_region`).
    if ctx.masked_region_scalar_locals.contains(&local_id) {
        return;
    }
    // The element-shape clone's preheader checked this accumulator's current
    // Number tag, and the matcher admits only numeric-preserving writes in a
    // call-free clone. Its old shadow value may remain conservatively rooted;
    // the slow clone resumes ordinary mirroring after the scoped fact is gone.
    if ctx
        .element_shape_loop_facts
        .iter()
        .any(|fact| fact.numeric_accumulator == local_id)
    {
        return;
    }
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    if ctx.persistent_shadow_slots.contains(&slot_idx) {
        if !expr_is_known_non_pointer_shadow_value(ctx, rhs) {
            let value_bits = ctx.block().bitcast_double_to_i64(value_reg);
            emit_persistent_shadow_root_barrier(ctx, &value_bits);
        }
        return;
    }
    if expr_is_known_non_pointer_shadow_value(ctx, rhs) {
        emit_shadow_slot_clear(ctx, slot_idx);
    } else {
        // Every caller has already stored the new value in the local alloca.
        // `js_shadow_slot_bind` copies that slot into the shadow frame, marks
        // it active, and runs the root barrier, so a following slot-set call
        // only repeated the same TLS lookup, copy, and barrier.
        emit_shadow_slot_bind_for_local(ctx, local_id);
    }
}
