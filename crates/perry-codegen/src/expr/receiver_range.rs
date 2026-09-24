//! The fused receiver test: "is this NaN-box a heap object pointer?" as ONE
//! unsigned range compare, shared by every inline property read and write
//! guard.
//!
//! A property access may dereference its receiver only when the value is
//! POINTER-tagged (`bits >> 48 == 0x7FFD`) AND its 48-bit payload lies above
//! the native-registry handle band (`payload > 0x0F_FFFF`: small ids that
//! native modules hand out as POINTER-tagged values and that are not
//! addresses). Spelled as two tests that is six instructions on x86-64 —
//! `xor`/`mov`/`shr $48`/`jne` and `cmp $0xfffff`/`jbe` — on every hit of every
//! read.
//!
//! The two tests are one range:
//!
//! ```text
//! t = bits - (POINTER_TAG | 0x10_0000)
//! t <u 2^48 - 0x10_0000     <=>     tag == 0x7FFD  &&  payload >= 0x10_0000
//! ```
//!
//! A higher tag leaves `t >= 2^48`; a lower tag, a small handle or a plain
//! double wraps `t` past `2^63`. Both fail the one compare, so it is exact —
//! [`tests::fused_test_equals_tag_and_handle_band_predicate`] checks every one
//! of the 65,536 tags against every payload boundary. On the pointer edge the
//! handle is `t + 0x10_0000`, and a load at a fixed offset from the handle is
//! a load at `offset + 0x10_0000` from `t` ([`emit_field_ptr`]), so the
//! handle itself need never be materialised on a hit path.
//!
//! #11161 introduced this compare for the class-field read route; this module
//! is where every route takes it from, so no two guards carry their own copy
//! of the constants.

use crate::block::LlBlock;
use crate::types::{I32, I64, I8};

/// Payloads below this are native-registry handles, never heap cells
/// (`js_native_call_method`'s small-handle test, `addr_class::HANDLE_BAND_MAX`).
pub(crate) const HANDLE_FLOOR: u64 = 0x10_0000;
/// `POINTER_TAG | HANDLE_FLOOR`: subtracting it maps exactly the heap-object
/// receivers onto `[0, RECEIVER_SPAN)`.
pub(crate) const RECEIVER_BIAS: u64 = crate::nanbox::POINTER_TAG | HANDLE_FLOOR;
/// `2^48 - HANDLE_FLOOR`.
pub(crate) const RECEIVER_SPAN: u64 = (1u64 << 48) - HANDLE_FLOOR;

const _: () = assert!(RECEIVER_BIAS == 0x7FFD_0000_0010_0000);
const _: () = assert!(RECEIVER_SPAN == 0x0000_FFFF_FFF0_0000);

/// The receiver test's two outputs: the biased value (`bits - RECEIVER_BIAS`)
/// every address on the pointer edge is derived from, and the `i1` that is
/// true exactly for a heap-object receiver.
pub(crate) struct FusedReceiver {
    pub(crate) biased: String,
    pub(crate) is_object_pointer: String,
}

/// Emit the fused receiver test on a receiver's raw bits, in the current
/// block. The caller branches on `is_object_pointer`; nothing on the pointer
/// edge may be addressed except through [`emit_handle`] or [`emit_field_ptr`].
pub(crate) fn emit_fused_receiver_test(blk: &mut LlBlock, bits: &str) -> FusedReceiver {
    let biased = blk.sub(I64, bits, &(RECEIVER_BIAS as i64).to_string());
    let is_object_pointer = blk.icmp_ult(I64, &biased, &(RECEIVER_SPAN as i64).to_string());
    FusedReceiver {
        biased,
        is_object_pointer,
    }
}

/// The receiver's handle (its 48-bit payload) from the biased value: on the
/// pointer edge `biased + HANDLE_FLOOR == bits ^ POINTER_TAG == bits &
/// POINTER_MASK`. Off it the value is meaningless and must not be addressed.
pub(crate) fn emit_handle(blk: &mut LlBlock, biased: &str) -> String {
    blk.add(I64, biased, &HANDLE_FLOOR.to_string())
}

/// A pointer to `handle + offset`, addressed from the biased value so the
/// `+ HANDLE_FLOOR` folds into the load's displacement instead of costing an
/// `add`/`lea` on the hit path. `offset` may be negative (GC header fields).
pub(crate) fn emit_field_ptr(blk: &mut LlBlock, biased: &str, offset: i64) -> String {
    let base = blk.inttoptr(I64, biased);
    let disp = (HANDLE_FLOOR as i64 + offset).to_string();
    blk.gep(I8, &base, &[(I64, &disp)])
}

/// The inline routes the admission census counts. **Must match
/// `RECV_ROUTE_NAMES` in perry-runtime's `hot_diag.rs`.**
#[derive(Clone, Copy)]
pub(crate) enum Route {
    /// A generic read (any key but `.length`) that passed the receiver test.
    Generic = 0,
    /// ...and was served by the compact MRU word.
    GenericMruHit = 1,
    /// ...and was served by one of the polymorphic ways.
    GenericWayHit = 2,
    /// A read region's receiver test passed (R1 part 1).
    Region = 3,
    /// A class-field read guard's receiver test passed.
    ClassRead = 4,
    /// A class-field write guard's receiver test passed.
    ClassWrite = 5,
    /// A `"k" in o` presence site's receiver test passed.
    InPresence = 6,
    /// The cached field-index early return's receiver test passed.
    CachedFieldIndex = 7,
}

/// `PERRY_RECV_ROUTE_COUNT=1` at COMPILE time: emit one
/// `js_recv_route_note(route)` call per route execution. A census-only build
/// knob; unset (the default), nothing is emitted and the routes are
/// byte-identical.
pub(crate) fn route_census_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_RECV_ROUTE_COUNT").is_ok_and(|v| v.trim() == "1"))
}

/// Count one execution of `route` in the current block (census builds only).
pub(crate) fn emit_route_note(blk: &mut LlBlock, route: Route) {
    if route_census_enabled() {
        blk.call_void("js_recv_route_note", &[(I32, &(route as u32).to_string())]);
    }
}

/// The emitted constants, for the IR-level tests of each route.
#[cfg(test)]
pub(crate) const RECEIVER_BIAS_LITERAL: &str = "9222527611925692416";
#[cfg(test)]
pub(crate) const RECEIVER_SPAN_LITERAL: &str = "281474975662080";

#[cfg(test)]
mod tests {
    use super::*;

    /// The predicate the two-test form spelled out.
    fn tag_and_handle_band(bits: u64) -> bool {
        (bits >> 48) == 0x7FFD && (bits & 0x0000_FFFF_FFFF_FFFF) >= HANDLE_FLOOR
    }

    /// The emitted form.
    fn fused(bits: u64) -> bool {
        bits.wrapping_sub(RECEIVER_BIAS) < RECEIVER_SPAN
    }

    /// The fused compare admits EXACTLY the receivers the tag test plus the
    /// small-handle test admitted: every tag (all 65,536 of them, so every
    /// NaN-box tag and every plain-double exponent pattern), each against the
    /// payload boundaries a one-off in either constant would move — zero, the
    /// top of the handle band and the first address above it, and the top of
    /// the 48-bit payload.
    #[test]
    fn fused_test_equals_tag_and_handle_band_predicate() {
        let payloads = [
            0u64,
            1,
            HANDLE_FLOOR - 2,
            HANDLE_FLOOR - 1,
            HANDLE_FLOOR,
            HANDLE_FLOOR + 1,
            0x7F12_3456_7890,
            0x0000_8000_0000_0000,
            0x0000_FFFF_FFFF_FFFE,
            0x0000_FFFF_FFFF_FFFF,
        ];
        let mut admitted = 0u32;
        for tag in 0u64..=0xFFFF {
            for payload in payloads {
                let bits = (tag << 48) | payload;
                assert_eq!(
                    fused(bits),
                    tag_and_handle_band(bits),
                    "tag {tag:#06x} payload {payload:#x}: the fused test disagrees"
                );
                admitted += u32::from(fused(bits));
            }
        }
        // Only the POINTER tag with a payload at or above the floor: six of
        // the payloads above, under exactly one tag. A test that admitted
        // nothing would pass the equality above for a broken predicate pair.
        assert_eq!(admitted, 6);
    }

    /// On the pointer edge the handle derived from the biased value is the
    /// payload, i.e. what `bits ^ POINTER_TAG` and `bits & POINTER_MASK`
    /// produced — at the floor, in the middle and at the top of the range.
    #[test]
    fn handle_from_the_biased_value_is_the_payload() {
        for payload in [HANDLE_FLOOR, 0x7F12_3456_7890, 0x0000_FFFF_FFFF_FFFF] {
            let bits = crate::nanbox::POINTER_TAG | payload;
            let biased = bits.wrapping_sub(RECEIVER_BIAS);
            assert!(biased < RECEIVER_SPAN);
            assert_eq!(biased + HANDLE_FLOOR, payload);
            assert_eq!(biased + HANDLE_FLOOR, bits ^ crate::nanbox::POINTER_TAG);
        }
    }

    #[test]
    fn emitted_literals_are_the_constants() {
        assert_eq!(RECEIVER_BIAS_LITERAL, (RECEIVER_BIAS as i64).to_string());
        assert_eq!(RECEIVER_SPAN_LITERAL, (RECEIVER_SPAN as i64).to_string());
    }
}
