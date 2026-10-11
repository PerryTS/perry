//! The method site's one cold exit (perry-codegen `expr/method_site.rs`).
//!
//! An emitted site tests its receiver once (the fused receiver test): a heap
//! object takes the site's memo, and every other receiver (a string, a number,
//! a boolean, `undefined`, a native handle) is not the site's to prime. Both
//! cold edges branch to ONE call, [`js_method_site_miss`]; it decides the
//! receiver kind again here, before the miss body's frame exists, so a
//! primitive other than a string pays a compare and a jump on top of the universal-dispatcher call
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
#[inline(never)]
pub unsafe extern "C-unwind" fn js_method_site_miss(
    slot: *mut MethodSiteSlot,
    site_id: u64,
    recv: f64,
    method_id: i64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    if !is_site_receiver(recv) {
        if crate::value::JSValue::from_bits(recv.to_bits()).is_any_string() {
            if let Some(result) = super::builtin_receiver::call_hit(slot, recv, args_ptr, argc)
                .or_else(|| {
                    super::builtin_receiver::call_miss(slot, recv, method_id, args_ptr, argc)
                })
            {
                return result;
            }
        }
        return crate::typed_feedback::js_typed_feedback_native_call_method_by_id(
            site_id, recv, method_id, args_ptr, argc,
        );
    }
    method_site_miss_object(slot, site_id, recv, method_id, args_ptr, argc)
}

// Small calls cross the generated/runtime boundary in argument registers.
// The shared bridge owns the stack buffer used by uncommon native-list bodies
// and the generic miss; no JS argument array or Rust vector is constructed.
/// Small-arity method dispatch. Safety: as `js_method_site_miss`.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_miss0(
    slot: *mut MethodSiteSlot,
    site: u64,
    recv: f64,
    method: i64,
) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    super::builtin_receiver::call_registers(
        slot, site, recv, method, 0, undefined, undefined, undefined,
    )
}

/// Small-arity method dispatch. Safety: as `js_method_site_miss`.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_miss1(
    slot: *mut MethodSiteSlot,
    site: u64,
    recv: f64,
    method: i64,
    a0: f64,
) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    super::builtin_receiver::call_registers(slot, site, recv, method, 1, a0, undefined, undefined)
}

/// Small-arity method dispatch. Safety: as `js_method_site_miss`.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_miss2(
    slot: *mut MethodSiteSlot,
    site: u64,
    recv: f64,
    method: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    super::builtin_receiver::call_registers(slot, site, recv, method, 2, a0, a1, undefined)
}

/// Small-arity method dispatch. Safety: as `js_method_site_miss`.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_site_miss3(
    slot: *mut MethodSiteSlot,
    site: u64,
    recv: f64,
    method: i64,
    a0: f64,
    a1: f64,
    a2: f64,
) -> f64 {
    super::builtin_receiver::call_registers(slot, site, recv, method, 3, a0, a1, a2)
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

    /// String primitives reuse the builtin holder proof and avoid the object
    /// miss body; non-string primitives remain outside the site's domain.
    #[test]
    fn string_receiver_uses_the_builtin_holder_without_an_object_miss() {
        if !super::super::run_with_fresh_worker_gate(
            "string_receiver_uses_the_builtin_holder_without_an_object_miss",
        ) {
            return;
        }
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
            assert!(!slot.is_null(), "the string builtin proof must be live");
            let short = f64::from_bits(crate::value::JSValue::short_string_unchecked(b"bb").bits());
            assert_eq!(
                js_method_site_miss1(&mut slot, 0, short, method_id, args[0]),
                0.0
            );
            assert_eq!(
                js_method_site_miss0(&mut slot, 0, string(b"abundefined"), method_id),
                2.0
            );
            assert_eq!(
                js_method_site_miss2(&mut slot, 0, recv, method_id, args[0], 2.0),
                3.0
            );
            assert_eq!(
                js_method_site_miss3(&mut slot, 0, recv, method_id, args[0], 0.0, 999.0),
                1.0
            );
            assert_eq!(
                super::super::builtin_receiver::call_hit(&mut slot, 7.0, args.as_ptr(), 1),
                None
            );
            // Negative control: a store to the holder must revoke the hit,
            // even when the receiver and the site's recorded body are unchanged.
            let holder = crate::array::primitive_wrapper_prototype_addr(recv)
                as *mut crate::object::ObjectHeader;
            let key = crate::string::canonical_key(b"indexOf");
            let selected = crate::object::js_object_get_field_by_name_f64(holder, key);
            crate::object::js_object_set_field_by_name(holder, key, 123.0);
            assert_eq!(
                super::super::builtin_receiver::call_hit(&mut slot, recv, args.as_ptr(), 1),
                None
            );
            crate::object::js_object_set_field_by_name(holder, key, selected);
            assert_eq!(
                super::super::method_site_stats().2,
                misses,
                "a primitive is not a site miss"
            );
        }
    }
}
