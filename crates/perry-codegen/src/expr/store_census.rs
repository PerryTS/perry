//! `PERRY_STORE_CENSUS=1` at COMPILE time: count, per route, how many stores
//! the program executes.
//!
//! Each counted route bumps one word of the runtime's `PERRY_STORE_CENSUS`
//! array with a plain load/add/store (no call, so nothing about the route's
//! GC or rooting changes). The runtime prints the non-zero counters at exit
//! when `PERRY_STORE_CENSUS` is also set at RUN time, next to the ones it
//! classifies itself (key-adds taken by the full `[[Set]]`, memo primes).
//! Off (the default) this emits nothing.
//!
//! **Indices must equal `perry_runtime::proxy::put_value::packed_add`'s
//! `CENSUS_NAMES`** (the runtime owns 16..32; the array-element counters are
//! 32..48).

use super::FnCtx;
use crate::types::I64;

/// Existing-key inline hit (the site word or an inline way).
pub(crate) const PIC_HIT: usize = 0;
/// Key-add inline hit.
pub(crate) const ADD_HIT: usize = 2;
/// The static-key store's miss call.
pub(crate) const PIC_MISS: usize = 3;
/// Class-field store: the shape-proven raw store (no runtime guard).
pub(crate) const CFIELD_SHAPE_PROVEN: usize = 4;
/// Class-field store: the inline-guarded fast store.
pub(crate) const CFIELD_GUARD_STORE: usize = 5;
/// Class-field store: the guard's fallback call.
pub(crate) const CFIELD_GUARD_FALLBACK: usize = 6;
/// Class-field store: a `js_class_field_set_ic` call.
pub(crate) const CFIELD_IC_CALL: usize = 7;
// Index 8 remains a zero tombstone for the removed class-loop raw store.
// The runtime's diagnostic array keeps its indices stable across the deletion.
/// Class setter dispatch (`__set_<name>`).
pub(crate) const CFIELD_SETTER: usize = 9;
/// Sloppy-mode class-field store.
pub(crate) const CFIELD_SLOPPY: usize = 10;
/// The runtime by-name store (no inline path).
pub(crate) const BY_NAME_RUNTIME: usize = 11;
/// The PutValue by-name lowering.
pub(crate) const BY_NAME_PUT_VALUE: usize = 12;
/// Key-add inline hit on one of the runtime block's first ways (counted on
/// its own edge, then also as [`ADD_HIT`]).
pub(crate) const ADD_WAY_HIT: usize = 14;

/// Array element read: the guarded tier's in-bounds, non-hole load.
pub(crate) const ELEM_READ_FAST: usize = 32;
/// Array element read: the hole / out-of-bounds arm (guard word passed).
pub(crate) const ELEM_READ_HOLE: usize = 33;
/// Array element read: the guard word failed (the cold arm).
pub(crate) const ELEM_READ_COLD: usize = 34;
/// Array element read: the runtime fallback call.
pub(crate) const ELEM_READ_FALLBACK: usize = 35;
/// Array element read served by another tier (trusted, region-validated).
pub(crate) const ELEM_READ_OTHER_TIER: usize = 36;
/// Array element store: the inline in-bounds overwrite.
pub(crate) const ELEM_STORE_INBOUNDS: usize = 37;
/// Array element store: the inline append at `index == length`.
pub(crate) const ELEM_STORE_APPEND: usize = 38;
/// Array element store: the inline guard word failed.
pub(crate) const ELEM_STORE_GUARD_MISS: usize = 39;
/// Array element store: a runtime set / extend call.
pub(crate) const ELEM_STORE_FALLBACK: usize = 40;
/// Array element read through a versioned-indexed loop fact specifically.
/// Unlike OTHER_TIER, no region or trusted-parameter read increments this word.
pub(crate) const ELEM_READ_VERSIONED_INDEXED: usize = 41;
/// Array element store into an F64 array of a NaN-boxed value: the cold arm
/// that clears the kind (header first) or converts an INT32 box.
pub(crate) const ELEM_STORE_F64_COLD: usize = 42;

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_STORE_CENSUS").as_deref() == Ok("1"))
}

/// Count one execution of the current block's route.
pub(crate) fn bump(ctx: &mut FnCtx<'_>, idx: usize) {
    if !enabled() {
        return;
    }
    let blk = ctx.block();
    let word = blk.gep(I64, "@PERRY_STORE_CENSUS", &[(I64, &idx.to_string())]);
    let n = blk.load(I64, &word);
    let n1 = blk.add(I64, &n, "1");
    // GC_STORE_AUDIT(POINTER_FREE): a counter in a runtime static.
    blk.store(I64, &n1, &word);
}
