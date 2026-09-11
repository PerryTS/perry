use super::*;

fn value(buf: *const BufferHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
}

#[test]
fn suffix_views_allocate_only_headers_and_share_native_bytes() {
    let source = js_uint8array_alloc(4096);
    let scope = crate::gc::RuntimeHandleScope::new();
    let _source = scope.root_raw_mut_ptr(source);
    for start in 0..4096 {
        let view = js_buffer_slice(source, start, 4096);
        unsafe {
            assert_eq!((*view).length, 4096 - start as u32);
            assert_eq!((*view).capacity, 0, "a view must not allocate its payload");
            let header = crate::value::addr_class::try_read_gc_header(view as usize).unwrap();
            assert_eq!(header.size as usize, crate::gc::GC_HEADER_SIZE + 8);
            assert_eq!(buffer_data(view), buffer_data(source).add(start as usize));
        }
        assert!(is_uint8array_buffer(view as usize));
    }
}

#[test]
fn overlapping_nested_views_share_every_write_path() {
    let source = js_buffer_alloc(12, 0);
    let a = js_buffer_slice(source, 2, 10);
    let b = js_buffer_slice(source, 4, 12);
    let nested = js_buffer_slice(a, 2, 6);
    js_buffer_set(source, 4, 17);
    assert_eq!(js_buffer_get(nested, 0), 17);
    js_buffer_set(nested, 1, 23);
    assert_eq!(js_buffer_get(source, 5), 23);
    assert_eq!(js_buffer_get(b, 1), 23);
    // Native writes and numeric accessors bypass indexed-write helpers.
    unsafe {
        *buffer_data_mut(b).add(2) = 31;
    }
    assert_eq!(js_buffer_get(a, 4), 31);
    js_buffer_write_uint16_le(value(nested), 0x1234 as f64, 0);
    assert_eq!(js_buffer_get(source, 4), 0x34);
    assert_eq!(js_buffer_get(b, 1), 0x12);
    assert_eq!(buffer_byte_offset(nested as usize), 4);
    assert_eq!(
        buffer_backing_array_buffer(a as usize),
        buffer_backing_array_buffer(b as usize)
    );
    assert_eq!(
        js_native_buffer_data_ptr(value(nested)),
        buffer_data(nested)
    );
    assert_eq!(js_native_buffer_byte_len(value(nested)), 4);
}

#[test]
fn empty_and_clamped_subarrays_preserve_backing_and_offsets() {
    let source = js_uint8array_alloc(8);
    let backing = buffer_backing_array_buffer(source as usize);
    for (start, end, offset, length) in [
        (-3, i32::MAX, 5, 3),
        (i32::MIN, 3, 0, 3),
        (20, 30, 8, 0),
        (6, 2, 6, 0),
        (-2, -1, 6, 1),
        (8, 8, 8, 0),
    ] {
        let view = js_buffer_slice(source, start, end);
        assert_eq!(unsafe { (*view).length }, length);
        assert_eq!(buffer_byte_offset(view as usize), offset);
        assert_eq!(buffer_backing_array_buffer(view as usize), backing);
        assert_eq!(buffer_data(view), unsafe {
            buffer_data(source).add(offset as usize)
        });
    }
}

#[test]
fn overlapping_buffer_copy_uses_memmove_semantics() {
    let source = js_buffer_alloc(8, 0);
    for i in 0..8 {
        js_buffer_set(source, i, i + 1);
    }
    let left = js_buffer_slice(source, 0, 6);
    let right = js_buffer_slice(source, 2, 8);
    assert_eq!(js_buffer_copy(left, right, 0, 0, 6), 6);
    for (i, byte) in [1, 2, 1, 2, 3, 4, 5, 6].iter().enumerate() {
        assert_eq!(js_buffer_get(source, i as i32), *byte);
    }
    js_buffer_set_from_value(left, value(right), 0.0);
    for (i, byte) in [1, 2, 3, 4, 5, 6, 5, 6].iter().enumerate() {
        assert_eq!(js_buffer_get(source, i as i32), *byte);
    }
}

#[test]
fn dead_view_removal_leaves_no_reverse_index_tombstones() {
    let source = js_buffer_alloc(1, 42);
    let before = view::registry_sizes();
    for _ in 0..1024 {
        let view = js_buffer_slice(source, 0, 1);
        header::finalize_collected_dead_buffer(view as usize);
    }
    assert_eq!(view::registry_sizes(), before);
    let live = js_buffer_slice(source, 0, 1);
    assert_eq!(js_buffer_get(live, 0), 42);
}

#[test]
fn detaching_materialized_arraybuffer_invalidates_all_shared_views() {
    let source = js_uint8array_alloc(16);
    let view = js_buffer_slice(source, 4, 12);
    let backing = buffer_backing_array_buffer(source as usize);
    let copy = buffer_slice_copy(backing as *const BufferHeader, 0, 16);
    mark_as_array_buffer(copy as usize);
    detach_array_buffer(backing);
    assert_eq!(unsafe { (*source).length }, 0);
    assert_eq!(unsafe { (*view).length }, 0);
    assert_eq!(unsafe { (*(backing as *const BufferHeader)).length }, 0);
    assert_eq!(buffer_byte_offset(view as usize), 0);
    assert_eq!(unsafe { (*copy).length }, 16);
}

#[test]
fn arraybuffer_and_uint8array_slice_copy_but_buffer_slice_shares() {
    for kind in 0..4 {
        let source = js_buffer_alloc(4, 7);
        match kind {
            0 => mark_as_array_buffer(source as usize),
            1 => mark_as_shared_array_buffer(source as usize),
            2 => mark_as_uint8array(source as usize),
            _ => (),
        }
        let args = [1.0, 3.0];
        let result = unsafe {
            crate::object::dispatch_buffer_method(source as usize, "slice", args.as_ptr(), 2)
        };
        let result = crate::value::JSValue::from_bits(result.to_bits()).as_pointer::<BufferHeader>()
            as *mut BufferHeader;
        js_buffer_set(source, 1, 9);
        assert_eq!(js_buffer_get(result, 0), if kind == 3 { 9 } else { 7 });
        js_buffer_set(result, 1, 11);
        assert_eq!(js_buffer_get(source, 2), if kind == 3 { 11 } else { 7 });
    }
}
