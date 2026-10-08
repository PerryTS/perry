use super::*;
use crate::gc::RuntimeHandleScope;
use crate::thread::{deserialize_nanbox_on_current_thread, serialize_message};
use crate::value::{JSValue, POINTER_MASK};
use std::sync::atomic::Ordering;

fn data(ptr: *const BufferHeader) -> *mut u8 {
    bytes::no_gc(|_| {
        bytes::span(crate::value::js_nanbox_pointer(ptr as i64), false)
            .unwrap()
            .ptr
    })
}

#[test]
fn adopted_vec_keeps_capacity_layout_and_visible_length() {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(&[1, 2, 3]);
    let pointer = bytes.as_ptr();
    let capacity = bytes.capacity();
    assert_eq!(std::mem::size_of::<backing::Backing>(), 16);
    let backing = backing::Backing::from_vec(bytes);
    assert_eq!(backing.data(), pointer as *mut u8);
    assert_eq!(backing.capacity() as usize, capacity);
    assert_eq!(unsafe { *backing.data().add(2) }, 3);
    drop(backing); // must free the Vec's original layout, once
    let empty = backing::Backing::from_vec(Vec::new());
    assert_eq!(empty.capacity(), 0);
    assert_eq!(empty.data().align_offset(8), 0);
}

#[test]
fn adopted_response_bytes_survive_views_transfer_gc_and_worker_exit() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let before = backing::LIVE_BACKINGS.load(Ordering::SeqCst);
    let scope = RuntimeHandleScope::new();
    let mut bytes = Vec::with_capacity(1024 * 1024);
    bytes.resize(1024 * 1024, 0);
    bytes[..3].copy_from_slice(&[37, 91, 7]);
    let original = bytes.as_ptr() as usize;
    let (value, pin) = bytes::new_bytes(
        bytes::Brand::ArrayBuffer,
        bytes.len(),
        bytes::Init::AdoptVec(bytes),
    );
    let source = JSValue::from_bits(value.to_bits())
        .as_pointer::<BufferHeader>()
        .cast_mut();
    drop(pin);
    let root = scope.root_raw_mut_ptr(source);
    unsafe {
        assert!(is_array_buffer(source as usize));
        assert_eq!(
            crate::buffer::store::length(source as usize) as u32,
            1024 * 1024
        );
        let value = f64::from_bits(JSValue::pointer(source.cast()).bits());
        let view = js_uint8array_new(value);
        let view_root = scope.root_raw_mut_ptr(view);
        assert_eq!(data(view) as usize, original);
        *data(view).add(2) = 11;
        crate::gc::js_gc_collect();
        let source = root.get_raw_mut_ptr::<BufferHeader>();
        let view = view_root.get_raw_mut_ptr::<BufferHeader>();
        assert_eq!(*data(view).add(2), 11);
        assert_eq!(data(source) as usize, original);
        let message = serialize_message(
            JSValue::pointer(source.cast()).bits(),
            &[source as usize],
            None,
        )
        .unwrap();
        assert!(is_detached_buffer(source as usize));
        assert_eq!(store::length(view as usize), 0);
        crate::gc::js_gc_collect();
        assert_eq!(backing::LIVE_BACKINGS.load(Ordering::SeqCst), before + 1);
        std::thread::spawn(move || {
            crate::gc::register_runtime_handle_root_scanner_for_tests();
            let scope = RuntimeHandleScope::new();
            let bits = deserialize_nanbox_on_current_thread(&message);
            let root = scope.root_nanbox_u64(bits);
            drop(message);
            crate::gc::js_gc_collect();
            let receiver = (root.get_nanbox_u64() & POINTER_MASK) as *const BufferHeader;
            assert_eq!(
                crate::buffer::store::length(receiver as usize) as u32,
                1024 * 1024
            );
            assert_eq!(data(receiver) as usize, original);
            assert_eq!(*data(receiver).add(2), 11);
        })
        .join()
        .unwrap();
    }
    assert_eq!(backing::LIVE_BACKINGS.load(Ordering::SeqCst), before);
}
