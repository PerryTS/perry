//! Allocation-free reads of an unchanged lazy JSON array.

use super::{JSValue, LazyArrayHeader};

/// Return a cached element, or materialize its subtree on first access.
///
/// The caller must supply a live lazy header. This fast path cannot allocate,
/// invoke user code or collect: it only loads the current cache slot. Rooting
/// is needed when constructing a value, not when returning an existing one.
#[inline]
pub unsafe fn lazy_get(hdr: *mut LazyArrayHeader, i: u32) -> JSValue {
    if hdr.is_null() {
        return JSValue::undefined();
    }
    // Mutation/full materialization takes precedence over both the sparse
    // cache and its length. The rooted accessor handles forwarding/getters.
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
    }
    super::lazy_get_rooted(hdr, i)
}

#[cfg(test)]
mod tests {
    use super::super::*;

    thread_local! {
        static ROOTED_READS: Cell<u32> = const { Cell::new(0) };
    }

    fn count_rooted_reads(point: JsonTapeSafepoint, _: usize) {
        if point == JsonTapeSafepoint::LazyGetHeaderRooted {
            ROOTED_READS.with(|n| n.set(n.get() + 1));
        }
    }

    struct HookGuard(Option<JsonTapeSafepointHook>);

    impl HookGuard {
        fn new() -> Self {
            ROOTED_READS.with(|n| n.set(0));
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
        let _hook = HookGuard::new();
        let input = format!(
            "[{}]",
            vec![r#"{"text":"heap string value"}"#; 130].join(",")
        );
        unsafe {
            let hdr = fixture(input.as_bytes());
            for (cold_reads, i) in [0, 63, 64, 65, 127, 128, 129].into_iter().enumerate() {
                let first = lazy_get(hdr, i);
                assert_eq!(ROOTED_READS.with(Cell::get), cold_reads as u32 + 1);
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
                assert_eq!(ROOTED_READS.with(Cell::get), cold_reads as u32 + 1);
            }
            assert!(lazy_get(std::ptr::null_mut(), 0).is_undefined());
        }
    }

    #[test]
    fn materialized_mutations_override_sparse_cache_and_original_length() {
        let _hook = HookGuard::new();
        unsafe {
            let hdr = fixture(b"[10,20,30]");
            assert_eq!(lazy_get(hdr, 1).as_number(), 20.0);
            assert_eq!(lazy_get(hdr, 1).as_number(), 20.0);
            assert_eq!(ROOTED_READS.with(Cell::get), 1);
            let arr = force_materialize_lazy(hdr);
            crate::array::js_array_set(arr, 1, JSValue::number(99.0));
            assert_eq!(lazy_get(hdr, 1).as_number(), 99.0);
            assert_eq!(ROOTED_READS.with(Cell::get), 2);
            let grown = crate::array::js_array_set_jsvalue_extend(arr, 7, JSValue::number(77.0).bits());
            assert!(!grown.is_null());
            assert_eq!(lazy_get(hdr, 7).as_number(), 77.0);
            assert!(lazy_get(hdr, 6).is_undefined());
            assert_eq!(ROOTED_READS.with(Cell::get), 4);
        }
    }
}
