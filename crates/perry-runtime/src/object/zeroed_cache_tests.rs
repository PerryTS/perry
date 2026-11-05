//! #11507: the per-thread transition cache is zero-allocated rather than
//! filled, so a thread's first view of it must be the empty entry everywhere.
//! (The array-tail tables' equivalent lives beside their miss encoding in
//! `array_tail_transition.rs`.)

use super::*;
use crate::codegen_abi::LayoutWord;

#[test]
fn fresh_thread_transition_cache_reads_empty_everywhere() {
    std::thread::spawn(|| {
        with_transition_cache(|table| unsafe {
            for entry in (*table).iter() {
                assert_eq!(
                    (
                        entry.key_ptr,
                        entry.next_keys,
                        entry.prev_shape_id,
                        entry.target_shape_id,
                        entry.slot_idx,
                        entry.target_len,
                    ),
                    (0, 0, LayoutWord(0), LayoutWord(0), 0, 0)
                );
            }
        });
    })
    .join()
    .unwrap();
}

/// Startup maps none of the fixed transition tables: the first use does.
#[test]
fn fixed_transition_tables_are_mapped_by_their_first_use() {
    std::thread::spawn(|| {
        crate::test_support::bootstrap_test_thread();
        let hot = &crate::state::state().object_hot;
        let unmapped = |hot: &crate::object::ObjectHotTables| {
            [
                hot.transition_cache.mapped().is_none(),
                hot.array_tail_forward.mapped().is_none(),
                hot.array_tail_reverse.mapped().is_none(),
                hot.array_tail_direct.mapped().is_none(),
            ]
        };
        assert_eq!(
            unmapped(hot),
            [true; 4],
            "startup mapped a transition table"
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let key = crate::string::js_string_from_bytes(b"fresh_key".as_ptr(), 9);
        crate::object::js_object_set_field_by_name(obj.get_raw_mut_ptr(), key, 1.0);
        assert!(
            hot.transition_cache.mapped().is_some(),
            "a dynamic add uses the cache"
        );
    })
    .join()
    .unwrap();
}
