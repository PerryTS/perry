//! Worker element brands are carried by their cells, without process caches.
use super::*;

#[test]
fn worker_and_main_thread_read_their_own_cell_brands() {
    let main = typed_array_alloc(KIND_UINT32, 16);
    js_typed_array_set(main, 0, 70000.0);
    let worker = std::thread::spawn(|| {
        let array = typed_array_alloc(KIND_UINT16, 16);
        js_typed_array_set(array, 0, 70000.0);
        assert_eq!(lookup_typed_array_kind(array as usize), Some(KIND_UINT16));
        js_typed_array_get(array, 0)
    })
    .join()
    .unwrap();
    assert_eq!(worker, 4464.0);
    assert_eq!(lookup_typed_array_kind(main as usize), Some(KIND_UINT32));
    assert_eq!(js_typed_array_get(main, 0), 70000.0);
}

#[test]
fn legacy_allocator_retirement_hook_has_no_live_admission_state_to_clear() {
    let first = typed_array_alloc(KIND_UINT32, 16);
    let second = typed_array_alloc(KIND_INT32, 16);
    invalidate_caches_in_range(first as usize, first as usize + 1);
    assert_eq!(
        owning_u32_addr(crate::value::js_nanbox_pointer(first as i64)),
        first as usize
    );
    assert_eq!(lookup_typed_array_kind(second as usize), Some(KIND_INT32));
}
