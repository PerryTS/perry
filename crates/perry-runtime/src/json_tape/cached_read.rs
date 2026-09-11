//! Allocation-free reads of existing lazy JSON array elements.

use super::{JSValue, LazyArrayHeader};

/// Return a cached element, or materialize its subtree on first access.
///
/// The caller must supply a live lazy header. This fast path cannot allocate,
/// invoke user code or collect: it resolves growth and loads an existing slot.
/// Cold reads, holes and descriptors keep the rooted general accessor.
// Keep the lazy hit/miss machinery outside the shared array dispatcher.
#[inline(never)]
pub unsafe fn lazy_get(hdr: *mut LazyArrayHeader, i: u32) -> JSValue {
    if hdr.is_null() {
        return JSValue::undefined();
    }
    // Mutation/full materialization takes precedence over both the sparse
    // cache and its length.
    if (*hdr).materialized.is_null() {
        if i >= (*hdr).cached_length {
            return JSValue::undefined();
        }
        let bitmap = (*hdr).materialized_bitmap;
        let cache = (*hdr).materialized_elements;
        if !bitmap.is_null()
            && !cache.is_null()
            && *bitmap.add(i as usize / 64) & (1u64 << (i % 64)) != 0
        {
            return *cache.add(i as usize);
        }
    } else {
        // This edge always names an ordinary array. Resolving its growth
        // forwarding cannot allocate or invoke user code; the fresh pointer
        // also satisfies array_object_flags_resolved's no-safepoint contract.
        let arr = super::resolve_materialized_array(hdr);
        if !arr.is_null()
            && crate::array::array_object_flags_resolved(arr)
                & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS
                == 0
            && i < (*arr).length
            && i < (*arr).capacity
        {
            let elements = (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *const u64;
            let bits = *elements.add(i as usize);
            // Holes must still consult prototypes; sparse and out-of-bounds
            // reads can do the same. Those paths may invoke a getter.
            if bits != crate::value::TAG_HOLE {
                return JSValue::from_bits(bits);
            }
        }
    }
    super::lazy_get_rooted(hdr, i)
}

#[cfg(test)]
mod tests {
    use super::super::*;

    use std::sync::atomic::{AtomicU32, Ordering};

    // The runtime suite is serial. This witness holds no managed values.
    static ROOTED_READS: AtomicU32 = AtomicU32::new(0);

    fn count_rooted_reads(point: JsonTapeSafepoint, _: usize) {
        if point == JsonTapeSafepoint::LazyGetHeaderRooted {
            ROOTED_READS.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct HookGuard(Option<JsonTapeSafepointHook>);

    impl HookGuard {
        fn install_counting_hook() -> Self {
            ROOTED_READS.store(0, Ordering::Relaxed);
            Self(test_set_safepoint_hook(Some(count_rooted_reads)))
        }
    }

    impl Drop for HookGuard {
        fn drop(&mut self) {
            test_set_safepoint_hook(self.0);
        }
    }

    unsafe fn fixture(input: &[u8]) -> *mut LazyArrayHeader {
        let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        with_built_tape(input, |tape| {
            alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
        })
        .unwrap()
    }

    #[test]
    fn cached_reads_preserve_identity_without_entering_rooted_construction() {
        let _hook = HookGuard::install_counting_hook();
        let input = format!(
            "[{}]",
            vec![r#"{"text":"heap string value"}"#; 130].join(",")
        );
        unsafe {
            let hdr = fixture(input.as_bytes());
            for (cold_reads, i) in [0, 63, 64, 65, 127, 128, 129].into_iter().enumerate() {
                let first = lazy_get(hdr, i);
                assert_eq!(ROOTED_READS.load(Ordering::Relaxed), cold_reads as u32 + 1);
                assert!(first.is_pointer(), "the identity subject must be an object");
                assert!(
                    (*hdr).materialized.is_null(),
                    "must exercise the sparse cache"
                );
                for _ in 0..10 {
                    assert_eq!(lazy_get(hdr, i).bits(), first.bits());
                    assert!(lazy_get(hdr, 130).is_undefined());
                    assert!(lazy_get(hdr, u32::MAX).is_undefined());
                }
                assert_eq!(ROOTED_READS.load(Ordering::Relaxed), cold_reads as u32 + 1);
            }
            assert!(lazy_get(std::ptr::null_mut(), 0).is_undefined());
        }
    }

    #[test]
    fn materialized_mutations_override_sparse_cache_and_original_length() {
        let _hook = HookGuard::install_counting_hook();
        unsafe {
            let hdr = fixture(b"[10,20,30]");
            assert_eq!(lazy_get(hdr, 1).as_number(), 20.0);
            assert_eq!(lazy_get(hdr, 1).as_number(), 20.0);
            assert_eq!(ROOTED_READS.load(Ordering::Relaxed), 1);
            let arr = force_materialize_lazy(hdr);
            crate::array::js_array_set(arr, 1, JSValue::number(99.0));
            assert_eq!(lazy_get(hdr, 1).as_number(), 99.0);
            assert_eq!(ROOTED_READS.load(Ordering::Relaxed), 1);
            let grown =
                crate::array::js_array_set_jsvalue_extend(arr, 7, JSValue::number(77.0).bits());
            assert!(!grown.is_null());
            assert_eq!(lazy_get(hdr, 7).as_number(), 77.0);
            assert_eq!(ROOTED_READS.load(Ordering::Relaxed), 1);
            assert!(lazy_get(hdr, 6).is_undefined());
            assert_eq!(ROOTED_READS.load(Ordering::Relaxed), 2);
        }
    }

    extern "C" fn descriptor_getter(_: *const crate::closure::ClosureHeader) -> f64 {
        61.0
    }

    #[test]
    fn materialized_descriptor_invokes_getter_through_rooted_fallback() {
        let _hook = HookGuard::install_counting_hook();
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let hdr = scope.root_raw_mut_ptr(fixture(b"[10,20,30]"));
            let arr = scope.root_raw_mut_ptr(hdr.with_mut_ptr(|hdr| force_materialize_lazy(hdr)));
            let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc(descriptor_getter as *const u8, 0) as i64,
            ));
            let desc = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            let key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
            desc.with_mut_ptr(|desc| {
                crate::object::js_object_set_field_by_name(desc, key, getter.get_nanbox_f64())
            });
            let key = crate::string::js_string_from_bytes(b"1".as_ptr(), 1);
            arr.with_mut_ptr::<crate::array::ArrayHeader, _>(|arr| {
                desc.with_mut_ptr::<crate::ObjectHeader, _>(|desc| {
                    crate::object::js_object_define_property(
                        crate::value::js_nanbox_pointer(arr as i64),
                        f64::from_bits(JSValue::string_ptr(key).bits()),
                        crate::value::js_nanbox_pointer(desc as i64),
                    )
                })
            });
            hdr.with_mut_ptr(|hdr| {
                let resolved = resolve_materialized_array(hdr);
                assert_ne!(
                    crate::array::array_object_flags_resolved(resolved)
                        & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
                    0,
                    "the real defineProperty path must install a descriptor"
                );
            });
            assert_eq!(ROOTED_READS.load(Ordering::Relaxed), 0);
            assert_eq!(hdr.with_mut_ptr(|hdr| lazy_get(hdr, 1)).as_number(), 61.0);
            assert_eq!(ROOTED_READS.load(Ordering::Relaxed), 1);
        }
    }
}
