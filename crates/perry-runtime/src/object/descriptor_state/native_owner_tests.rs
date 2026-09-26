//! A descriptor owner can be a native allocation whose preceding bytes decode
//! as a cell header. The install must prove ownership before it WRITES the
//! cell's meta edge.

use super::*;

/// Sabotage: dropping the tracked gate in `descriptor_summary_meta_ensure`
/// lets the install write a meta pointer into the native buffer, and the last
/// assertion fails.
#[test]
fn a_native_owner_that_decodes_as_a_map_keeps_its_bytes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let meta_word = std::mem::offset_of!(crate::map::MapHeader, meta) / 8;
    // Word 0 decodes as a Map header of plausible size; the "cell" starts at
    // word 1, an ordinary Rust heap allocation.
    let mut buf = vec![0u64; 16];
    buf[0] = u64::from(crate::gc::GC_TYPE_MAP) | (64u64 << 32);
    let owner = buf.as_ptr() as usize + 8;
    assert!(
        crate::value::addr_class::is_plausible_heap_addr(owner),
        "premise: the native buffer is heap-plausible"
    );
    set_accessor_descriptor(
        owner,
        "k".to_string(),
        AccessorDescriptor { get: 0, set: 0 },
    );
    assert!(get_handle_accessor_descriptor(owner, "k").is_some());
    clear_accessor_descriptor(owner, "k");
    assert_eq!(
        buf[1 + meta_word],
        0,
        "the install must not write a meta edge into a native allocation"
    );
}
