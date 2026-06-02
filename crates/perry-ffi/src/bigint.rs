//! BigInt surface — `perry-ffi`'s canonical `BigIntHeader` plus a
//! thin allocator that wrappers can use to construct
//! arbitrary-precision integers without touching runtime internals
//! directly.
//!
//! # Why
//!
//! Wrappers like `ethers` (parseUnits / formatUnits) and the
//! database drivers (postgres `int8` / `numeric` columns coming
//! back from spawn_blocking) need to return JS BigInts to user
//! code. Before v0.5.556 every such wrapper had to import
//! `perry_runtime::{BigIntHeader, js_bigint_from_string}` directly,
//! which made any internal renumbering of perry-runtime a
//! cross-cutting breaking change for every wrapper that touched
//! large integers.
//!
//! Today's surface is intentionally minimal: expose the ABI type plus
//! a single string-parsing constructor (which is what every existing
//! wrapper uses). Extras (limb-based
//! constructors, arithmetic ops, string-radix parsing) wait until
//! a real wrapper demands them.

use crate::{BigIntHeader, BIGINT_LIMBS};

extern "C" {
    /// Parse a decimal-string representation into a fresh
    /// `BigIntHeader` allocated in the runtime arena. Negative
    /// values are encoded in two's complement across all
    /// `BIGINT_LIMBS` u64 limbs. Invalid UTF-8 / non-decimal
    /// characters fall back to zero (matching perry-stdlib's
    /// existing convention).
    fn js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader;
}

/// Allocate a `BigIntHeader` from a Rust `&str` decimal
/// representation.
///
/// ```ignore
/// // formatEther / formatUnits style:
/// let big = perry_ffi::alloc_bigint_from_str("1500000000000000000");
/// ```
pub fn alloc_bigint_from_str(decimal: &str) -> *mut BigIntHeader {
    // SAFETY: `js_bigint_from_string` accepts any (`*const u8`, `u32`)
    // pair — it borrows the slice for the duration of the call.
    unsafe { js_bigint_from_string(decimal.as_ptr(), decimal.len() as u32) }
}

/// Read the raw 16-limb little-endian array out of a runtime-
/// allocated `BigIntHeader`. Returns `None` on a null pointer.
///
/// ```ignore
/// // Walk the limbs to render decimal:
/// if let Some(limbs) = perry_ffi::read_bigint_limbs(big_ptr) {
///     for (i, l) in limbs.iter().enumerate() {
///         println!("limb[{i}] = {l:#x}");
///     }
/// }
/// ```
pub fn read_bigint_limbs(ptr: *const BigIntHeader) -> Option<[u64; BIGINT_LIMBS]> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller's contract — `ptr` is a valid runtime-allocated
    // BigIntHeader. The struct layout is `#[repr(C)]` and the limbs
    // field is the only field, so the read-by-value never touches
    // unaligned memory.
    Some(unsafe { (*ptr).limbs })
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;

    #[test]
    fn round_trip_small_decimal() {
        let p = alloc_bigint_from_str("1234567890");
        let limbs = read_bigint_limbs(p).expect("non-null");
        // Small enough to fit in the bottom limb.
        assert_eq!(limbs[0], 1234567890u64);
        for &l in &limbs[1..] {
            assert_eq!(l, 0);
        }
    }

    #[test]
    fn zero_round_trips() {
        let p = alloc_bigint_from_str("0");
        let limbs = read_bigint_limbs(p).expect("non-null");
        for &l in &limbs {
            assert_eq!(l, 0);
        }
    }

    #[test]
    fn null_returns_none() {
        assert!(read_bigint_limbs(std::ptr::null()).is_none());
    }
}
