//! #10593: the inline array-index guards' "no inherited index can intercept
//! this access" test.
//!
//! An element access may take a raw-slot fast path only while no prototype
//! on the receiver's chain can carry an indexed property. Two facts decide
//! that, and every guard needs both:
//!
//! * the process-wide byte `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`: an index
//!   installed on `Array.prototype` / `Object.prototype`, or a retargeted
//!   `Array.prototype` — changes to the DEFAULT chain every array shares;
//! * the receiver's own `GC_ARRAY_CUSTOM_PROTO` bit (perry-runtime
//!   `gc/types.rs`): `Object.setPrototypeOf(thisArray, p)`.
//!
//! The second used to be folded into the first, so one retargeted array
//! anywhere stood every array's fast path down for the rest of the process —
//! including the element store's cheap barrier, which made every element store
//! in the program take the full old-to-young write barrier (33x whole-program
//! on the issue's fixture).

use crate::block::LlBlock;
use crate::types::{I1, I16, I8};

/// `GC_ARRAY_CUSTOM_PROTO` (bit 6 of a `GC_TYPE_ARRAY` header's `_reserved`
/// word). MUST match perry-runtime `gc/types.rs`.
pub(crate) const GC_ARRAY_CUSTOM_PROTO_I16: &str = "64";

/// Emit the i1 "the receiver's prototype chain is the pristine default one"
/// test for a `GC_TYPE_ARRAY` receiver whose `_reserved` word is `reserved`
/// (an `i16` already loaded by the caller's header guard).
pub(crate) fn emit_array_default_prototype_chain(blk: &mut LlBlock, reserved: &str) -> String {
    let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let globally_clean = blk.icmp_eq(I8, &invalidated, "0");
    let custom_proto_bit = blk.and(I16, reserved, GC_ARRAY_CUSTOM_PROTO_I16);
    let own_proto_default = blk.icmp_eq(I16, &custom_proto_bit, "0");
    blk.and(I1, &globally_clean, &own_proto_default)
}
