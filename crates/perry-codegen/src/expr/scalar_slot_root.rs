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
//! The bind is not repeated per alloca-per-store *shape*, only per store
//! *site*: the alloca is entry-hoisted and never moves, so one bind covers
//! the rest of the frame's life. Re-binding at a later store to the same
//! field is what re-runs the incremental-mark root barrier, which is
//! required for the same reason an ordinary local's re-assignment re-binds.
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

use crate::types::{I32, PTR};

/// Root the scalar-replacement alloca `slot` against the value expression
/// that was just stored into it.
///
/// Call *after* the `store` — `js_shadow_slot_bind` reads the alloca to seed
/// the shadow mirror and to run the root write barrier, so the new value has
/// to be in place. Callers that store a canonicalized raw `f64` (the
/// `numeric_store` arm of `expr::property_set`) must not call this at all:
/// those bits are a plain double by construction, and the shared root-word
/// decoder rejects them, but reserving a slot for them would be pure waste.
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
    let slot_idx = match ctx.scalar_slot_shadow_slots.get(slot).copied() {
        Some(idx) => idx,
        None => {
            // `None` means shadow-stack emission is off for this build; the
            // caller must not emit slot traffic either.
            let Some(idx) = ctx.func.reserve_shadow_slot() else {
                return;
            };
            ctx.scalar_slot_shadow_slots.insert(slot.to_string(), idx);
            idx
        }
    };
    ctx.block().call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx.to_string()), (PTR, slot)],
    );
}
