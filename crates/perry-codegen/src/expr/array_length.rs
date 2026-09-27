//! `.length` of an array handle, split into a GC-leaf fast path and a
//! collecting slow path (#11522).
//!
//! `js_array_length` used to be classified allocate-but-never-reenter, so under
//! the safepoint-only contract every call was a `gc-leaf-function` and the
//! caller's live GC values were not relocated across it. That was false: its
//! Proxy arm runs the user's `get` trap (#5135, immer drafts) and its
//! array-like-object arm runs getters plus `valueOf`/`Symbol.toPrimitive`
//! coercion. A collection inside any of them left the caller holding
//! from-space pointers.
//!
//! The fix keeps the plain-array fast lane statepoint-free without lying about
//! the rest: `js_array_length_leaf` (header reads only, `CannotCollect`)
//! answers the length or `-1`, and only a `-1` takes the ordinary
//! `js_array_length` call, which is now `Unknown` and so gets a real
//! statepoint and root reloads.

use crate::expr::FnCtx;
use crate::types::{I32, I64};

/// Emit the length of `handle` (a raw or POINTER-tagged `i64` array handle)
/// and return the `i32` SSA value holding it. Leaves `ctx` positioned in the
/// join block.
pub(crate) fn emit_array_length_i32(ctx: &mut FnCtx<'_>, handle: &str) -> String {
    let blk = ctx.block();
    let leaf = blk.call(I64, "js_array_length_leaf", &[(I64, handle)]);
    let miss = blk.icmp_slt(I64, &leaf, "0");
    let fast_len = blk.trunc(I64, &leaf, I32);
    let fast_end = blk.label.clone();
    let slow_idx = ctx.new_block("alen.slow");
    let done_idx = ctx.new_block("alen.done");
    let slow_label = ctx.block_label(slow_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&miss, &slow_label, &done_label);

    ctx.current_block = slow_idx;
    let slow_len = ctx.block().call(I32, "js_array_length", &[(I64, handle)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    ctx.block()
        .phi(I32, &[(&fast_len, &fast_end), (&slow_len, &slow_end)])
}
