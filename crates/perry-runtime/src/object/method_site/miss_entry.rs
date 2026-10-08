//! The method site's one cold exit (perry-codegen `expr/method_site.rs`).
//!
//! An emitted site tests its receiver once (the fused receiver test): a heap
//! object takes the site's memo, and every other receiver (a string, a number,
//! a boolean, `undefined`, a native handle) is not the site's to prime. Both
//! cold edges branch to ONE call, [`js_method_site_miss`]; it decides the
//! receiver kind again here, before the miss body's frame exists, so a
//! primitive pays a compare and a jump on top of the universal-dispatcher call
//! the site's separate primitive arm used to make, and that arm (a second copy
//! of the same operand setup at every method site) is gone.

use super::{method_site_miss_object, MethodSiteSlot};

/// The emitted fused receiver test (perry-codegen
/// `receiver_range::emit_fused_receiver_test`): a POINTER-tagged value whose
/// payload is above the native-handle band, i.e. a heap object. One wrapping
/// subtract and one unsigned compare, with the same constants.
#[inline(always)]
pub(super) fn is_site_receiver(recv: f64) -> bool {
    const BIAS: u64 = crate::value::POINTER_TAG | perry_abi::RECEIVER_HANDLE_FLOOR as u64;
    const SPAN: u64 = (1u64 << 48) - perry_abi::RECEIVER_HANDLE_FLOOR as u64;
    recv.to_bits().wrapping_sub(BIAS) < SPAN
}

/// The site's miss: a heap object goes to the priming miss body, anything else
/// straight to the universal dispatcher (module docs).
///
/// # Safety
/// `slot` is null or a live method-site slot; `args_ptr` holds `argc` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_miss(
    slot: *mut MethodSiteSlot,
    site_id: u64,
    recv: f64,
    method_id: i64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    if !is_site_receiver(recv) {
        return crate::typed_feedback::js_typed_feedback_native_call_method_by_id(
            site_id, recv, method_id, args_ptr, argc,
        );
    }
    method_site_miss_object(slot, site_id, recv, method_id, args_ptr, argc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runtime test is the emitted one: exactly the heap-object payloads
    /// pass, every primitive tag and the native-handle band do not.
    #[test]
    fn site_receiver_test_matches_the_emitted_fused_test() {
        let ptr = |payload: u64| f64::from_bits(crate::value::POINTER_TAG | payload);
        let floor = perry_abi::RECEIVER_HANDLE_FLOOR as u64;
        assert!(is_site_receiver(ptr(floor)));
        assert!(is_site_receiver(ptr(0x0000_FFFF_FFFF_FFFF)));
        assert!(
            !is_site_receiver(ptr(floor - 1)),
            "a native handle is not a site receiver"
        );
        assert!(!is_site_receiver(ptr(0)));
        for bits in [
            crate::value::TAG_UNDEFINED,
            crate::value::TAG_NULL,
            crate::value::TAG_TRUE,
            crate::value::TAG_FALSE,
            1.5f64.to_bits(),
            0x7FFF_0000_0010_0000, // a heap STRING
            0x7FF9_0000_0000_0003, // an SSO string
            0x7FFE_0000_0000_0007, // an INT32
        ] {
            assert!(!is_site_receiver(f64::from_bits(bits)), "{bits:#x}");
        }
    }

    /// A primitive receiver reaches the universal dispatcher and never the
    /// priming miss body: the answer is the dispatcher's, the site's slot
    /// stays empty, and no miss is counted.
    #[test]
    fn a_primitive_receiver_never_reaches_the_miss_body() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let _no_gc = crate::gc::GcSuppressScope::new();
            let string = |b: &[u8]| {
                crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                    b.as_ptr(),
                    b.len() as u32,
                ) as i64)
            };
            let recv = string(b"abcb");
            let method_id = string(b"indexOf").to_bits() as i64;
            let args = [string(b"b")];
            let (_, _, misses) = super::super::method_site_stats();
            let mut slot: MethodSiteSlot = std::ptr::null_mut();
            let answer = js_method_site_miss(&mut slot, 0, recv, method_id, args.as_ptr(), 1);
            assert_eq!(answer, 1.0, "the dispatcher's answer");
            assert!(slot.is_null(), "a primitive must not prime the site");
            assert_eq!(
                super::super::method_site_stats().2,
                misses,
                "a primitive is not a site miss"
            );
        }
    }
}
