//! GC rooting for scalar-replaced object fields and array elements (#6968).
//!
//! # The hole this closes
//!
//! Scalar replacement turns `const o = { a: fresh(), b: n }` into one
//! entry-block alloca per field and deletes the object. Those allocas belong
//! to no HIR local, so `collect_pointer_typed_locals` — which assigns shadow
//! slots by walking `Stmt::Let` — never sees them, and nothing ever calls
//! `js_shadow_slot_bind` for them:
//!
//! ```llvm
//!   %r13 = call double @perry_fn_m__fresh(double 0.0)
//!   store double %r13, ptr %r10          ; o.a — a bare, unrooted alloca
//!   %r16 = call double @perry_fn_m__churn(double %r15)  ; collects; %r10 swept
//! ```
//!
//! The object *local* does get a slot reserved (it is pointer-typed), but
//! lowering only ever clears it — there is no object handle to bind, which is
//! why #6951/#6972's object-literal rooting does not reach this shape.
//!
//! Until #6977 this was invisible: `gc_check_trigger` forces a conservative
//! native-stack scan, which finds the alloca. With precise roots only
//! (`PERRY_CONSERVATIVE_STACK_SCAN=off`) the value is swept out from under
//! the alloca and the program reads freed memory.
//!
//! # What is emitted
//!
//! At each store into such an alloca whose value may be a heap reference:
//! reserve one shadow-frame slot for that alloca (once) and bind it, exactly
//! as `expr::emit_shadow_slot_update_for_expr` does for an ordinary
//! pointer-typed local. `js_shadow_slot_bind` records `slot_ptrs[slot] =
//! alloca`, so both a mark-sweep root walk and an evacuating minor's
//! rewrite pass reach the real alloca rather than a stale mirror.
//!
//! The bind runs **once, in the function-entry setup** — not at each store.
//! What a bind does is `slot_ptrs[idx] = alloca; stack[idx] = *alloca;
//! active[idx] = true; root_barrier(*alloca)`. For an entry-hoisted alloca the
//! first three are loop-invariant: the address never changes, and every reader
//! of a bound slot (`visit_shadow_stack_root_slots`, `js_shadow_slot_get`)
//! dereferences `slot_ptrs[idx]` in preference to the `stack[idx]` mirror, so
//! the mirror is dead storage. Only the root barrier is per-store work, and it
//! is emitted inline and guarded (`emit_persistent_shadow_root_barrier`).
//!
//! This is the same treatment `enable_persistent_shadow_slot_for_array_alias`
//! already gives a `const item = arr[i]` alias, for the same reason.
//!
//! The hoist does **not** move when the rooted value is read. The collector
//! reads the alloca at collection time, exactly as it did when the bind sat at
//! the store; nothing is snapshotted into a register and re-read later. What
//! disappears is a redundant copy, not an observation point.
//!
//! # Why the slot must be initialized before the bind
//!
//! Binding at entry makes the slot `active` from function entry, so the
//! collector starts dereferencing the alloca *before* any store reaches it. An
//! uninitialized alloca would hand the root-word decoder stack garbage that
//! can pass `is_plausible_heap_addr`. Every path that hoists a bind therefore
//! initializes its alloca to `undefined` in `entry_allocas`, ahead of the
//! `entry_post_init_setup` region the bind lands in.
//!
//! # The gate
//!
//! Emission is skipped when the stored value cannot be a heap reference.
//! That decision is made from the **lowering**, never the declared type
//! (#6997): the predicate is `expr_is_known_non_pointer_shadow_value`, the
//! very one that decides whether an ordinary pointer local's slot is bound
//! or cleared. A field whose values are all numbers therefore costs nothing
//! — no slot, no call, no growth of the frame.

use super::*;

use perry_hir::Expr;

use crate::types::{I32, I64, PTR};

/// Root the scalar-replacement alloca `slot` against the value expression
/// that was just stored into it.
///
/// Call *after* the `store` — the emitted root barrier reads the alloca back,
/// so the new value has to be in place. Callers that store a canonicalized raw
/// `f64` (the `numeric_store` arm of `expr::property_set`) must not call this
/// at all: those bits are a plain double by construction, and the shared
/// root-word decoder rejects them, but reserving a slot for them would be pure
/// waste.
pub(crate) fn root_scalar_replaced_slot(ctx: &mut FnCtx<'_>, slot: &str, value: &Expr) {
    if expr_is_known_non_pointer_shadow_value(ctx, value) {
        return;
    }
    bind_scalar_replaced_slot(ctx, slot);
}

/// Root a scalar-replacement alloca whose stored value has no HIR expression
/// to gate on because codegen synthesized it.
///
/// Used by the scalar-replaced `String.prototype.split` arm, whose element
/// slots receive `js_string_split_part_value` results — heap strings with
/// nothing else referring to them.
pub(crate) fn root_scalar_replaced_slot_unconditional(ctx: &mut FnCtx<'_>, slot: &str) {
    bind_scalar_replaced_slot(ctx, slot);
}

fn bind_scalar_replaced_slot(ctx: &mut FnCtx<'_>, slot: &str) {
    if !ctx.scalar_slot_shadow_slots.contains_key(slot) {
        // `None` means shadow-stack emission is off for this build; the
        // caller must not emit slot traffic either.
        let Some(idx) = ctx.func.reserve_shadow_slot() else {
            return;
        };
        ctx.scalar_slot_shadow_slots.insert(slot.to_string(), idx);
        // One bind for the whole frame. `reserve_shadow_slot` runs first so a
        // lazily-created `js_shadow_frame_push` is already in
        // `entry_post_init_setup` when this call is appended after it — a bind
        // that ran before the push would land in the caller's frame.
        ctx.func.entry_setup_call_void(
            "js_shadow_slot_bind",
            &[(I32, &idx.to_string()), (PTR, slot)],
        );
    }
    emit_scalar_slot_store_barrier(ctx, slot);
}

/// The per-store remainder of a bind: shade the newly stored value so an
/// in-flight incremental mark cannot miss it.
///
/// The operand is read back from the alloca rather than threaded down from the
/// caller's value register **because that is precisely what the bind it
/// replaces did** (`js_shadow_slot_bind` dereferences `value_slot`). The load
/// sits in the same block, immediately after the store that produced the value,
/// with nothing in between — it cannot observe a later write, and LLVM forwards
/// it to the stored register.
fn emit_scalar_slot_store_barrier(ctx: &mut FnCtx<'_>, slot: &str) {
    let value_bits = ctx.block().load(I64, slot);
    crate::expr::emit_persistent_shadow_root_barrier(ctx, &value_bits);
}
