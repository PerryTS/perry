//! #11507: the per-thread transition cache is zero-allocated rather than
//! filled, so a thread's first view of it must be the empty entry everywhere.
//! (The array-tail tables' equivalent lives beside their miss encoding in
//! `array_tail_transition.rs`.)

use super::*;

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
                    (0, 0, 0, 0, 0, 0)
                );
            }
        });
    })
    .join()
    .unwrap();
}
