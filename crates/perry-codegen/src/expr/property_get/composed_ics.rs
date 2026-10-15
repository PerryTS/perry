//! Composed inline caches for property reads: the Symbol-then-named-field IC
//! and the object-backed Array-subclass `.length` tier.
//!
//! Child module of `property_get.rs`, split out to stay under the 2,000-line
//! file gate; `use super::*` keeps the parent's private helpers reachable.

use super::*;

/// Emit the object-backed Array-subclass tier for a guarded `.length` miss.
///
/// Cache words are scalar facts only: exact `(class, ShapeId)` or the stable
/// named-prefix token, the `length` slot, and the live inline-slot bound.  A
/// hit reloads every object/meta/spill pointer from the current receiver, so a
/// moving collection never needs to visit the per-site global.
pub(super) fn emit_array_subclass_length_ic(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    _recv_bits: &str,
    _recv_handle: &str,
    outer_merge_label: &str,
) -> (String, String) {
    // Every receiver the inline typed-array tier did not prove takes the
    // exact length helper (ordinary property semantics; no site state).
    emit_record_length_cold(ctx, recv_box, outer_merge_label)
}

/// A record's compact hot guard proves only an ordinary array. Every other
/// receiver, including typed arrays, takes the one shared cold body.
fn emit_record_length_cold(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    outer_merge_label: &str,
) -> (String, String) {
    let length = ctx.block().call(
        DOUBLE,
        "js_value_length_property_f64",
        &[(DOUBLE, recv_box)],
    );
    let end = ctx.block().label.clone();
    ctx.block().br(outer_merge_label);
    (length, end)
}

/// Array length uses the same compact header guard for records and ordinary
/// receivers. Every miss calls the existing generic property helper.
pub(super) fn emit_record_length(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    recv_bits: &str,
    recv_handle: &str,
) -> String {
    // The ordinary Array arm needs only POINTER_TAG. Preserve strings and
    // every other receiver through the shared exact length helper.
    let base = crate::nanbox::i64_literal(crate::expr::receiver_range::RECEIVER_BIAS);
    let offset = ctx.block().sub(I64, recv_bits, &base);
    let valid = ctx.block().icmp_ult(
        I64,
        &offset,
        crate::expr::receiver_range::RECEIVER_SPAN_LITERAL,
    );
    let check = ctx.new_block("plen.check_gc");
    let fast = ctx.new_block("plen.fast");
    let slow = ctx.new_block("plen.slow");
    let merge = ctx.new_block("plen.merge");
    let cl = ctx.block_label(check);
    let fl = ctx.block_label(fast);
    let sl = ctx.block_label(slow);
    let ml = ctx.block_label(merge);
    ctx.block().cond_br(&valid, &cl, &sl);
    ctx.current_block = check;
    // Consume the caller materialization. Reconstructing it from the band
    // offset prevents reuse with adjacent checked array operations.
    let handle = recv_handle.to_owned();
    let header = ctx.block().sub(I64, &handle, "8");
    let header = ctx.block().inttoptr(I64, &header);
    let word = ctx.block().load(crate::types::I16, &header);
    let kind = ctx.block().and(crate::types::I16, &word, "33023");
    let live = ctx.block().icmp_eq(crate::types::I16, &kind, "1");
    ctx.block().cond_br(&live, &fl, &sl);
    ctx.current_block = fast;
    let ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &ptr);
    let length = ctx.block().uitofp(I32, &length, DOUBLE);
    let fp = ctx.block().label.clone();
    ctx.block().br(&ml);
    ctx.current_block = slow;
    let (cold, sp) = emit_record_length_cold(ctx, recv_box, &ml);
    ctx.current_block = merge;
    ctx.block().phi(DOUBLE, &[(&length, &fp), (&cold, &sp)])
}
