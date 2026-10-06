use super::*;

fn value(buf: *const BufferHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
}

#[test]
fn suffix_views_are_sixteen_byte_cells_and_share_native_bytes() {
    let source = js_uint8array_alloc(4096);
    let scope = crate::gc::RuntimeHandleScope::new();
    let _source = scope.root_raw_mut_ptr(source);
    for start in 0..4096 {
        let view = js_buffer_slice(source, start, 4096);
        unsafe {
            assert_eq!(
                super::store::length(view as usize) as u32,
                4096 - start as u32
            );
            assert_eq!((*view).capacity, start as u32);
            let header = crate::value::addr_class::try_read_gc_header(view as usize).unwrap();
            assert_eq!(
                header.size as usize,
                crate::gc::GC_HEADER_SIZE + crate::codegen_abi::BYTES_STORE
            );
            assert!(crate::gc::is_byte_view_type(header.obj_type));
            bytes::no_gc(|_| {
                assert_eq!(
                    bytes::span(value(view), false).unwrap().ptr,
                    bytes::span(value(source), false)
                        .unwrap()
                        .ptr
                        .add(start as usize)
                );
            });
        }
        assert!(is_uint8array_buffer(view as usize));
    }
}

#[test]
fn byte_views_follow_the_flattened_owner_without_address_caches() {
    let owner = js_uint8array_alloc(32);
    let sub = js_buffer_slice(owner, 4, 20);
    let nested = js_buffer_slice(sub, 3, 9);
    js_buffer_set(owner, 7, 91);
    let lease = bytes::pin(value(owner)).unwrap();
    let expected = unsafe { lease.as_ptr().add(7) };
    for _ in 0..128 {
        assert_eq!(js_buffer_get(nested, 0), 91);
        assert_eq!(js_buffer_index_get_value(nested, 0), 91.0);
        assert_eq!(
            js_buffer_index_get_value(nested, 6).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        bytes::no_gc(|_| {
            assert_eq!(
                bytes::span(value(nested), false).unwrap().ptr as *const u8,
                expected
            )
        });
    }
    assert_eq!(unsafe { (*nested).link }, owner as usize);
}

#[test]
fn rewritten_view_backing_refreshes_the_derived_pointer() {
    let owner = js_uint8array_alloc(32);
    let replacement = js_uint8array_alloc(32);
    js_buffer_set(owner, 5, 17);
    js_buffer_set(replacement, 5, 29);
    let sub = js_buffer_slice(owner, 5, 15);
    assert_eq!(js_buffer_get(sub, 0), 17);
    unsafe {
        (*sub).link = replacement as usize;
    }
    assert_eq!(js_buffer_get(sub, 0), 29);
    bytes::no_gc(|_| unsafe {
        assert_eq!(
            bytes::span(value(sub), false).unwrap().ptr,
            bytes::span(value(replacement), false).unwrap().ptr.add(5)
        );
    });
    unsafe {
        (*sub).link = owner as usize;
    }
}

#[test]
fn data_view_has_owner_link_and_byte_offset() {
    let backing = js_array_buffer_new(16);
    let backing_value = value(backing);
    let view_value = js_data_view_new(backing_value, 4.0, 8.0);
    let view = crate::value::JSValue::from_bits(view_value.to_bits()).as_pointer::<BufferHeader>()
        as *mut BufferHeader;

    unsafe {
        assert_eq!(super::store::length(view as usize) as u32, 8);
        assert_eq!((*view).capacity, 4);
        bytes::no_gc(|_| {
            assert_eq!(
                view::data_view_data_ptr(view),
                bytes::span(backing_value, false).unwrap().ptr.add(4)
            )
        });
    }

    js_data_view_set(
        view_value,
        0.0,
        0x0102_0304 as f64,
        DataViewKind::Uint32,
        false,
    );
    bytes::no_gc(|scope| {
        assert_eq!(
            &bytes::bytes(backing_value, scope).unwrap()[4..8],
            &[1, 2, 3, 4]
        )
    });
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
    bytes::no_gc(|scope| unsafe {
        bytes::bytes_mut(value(b), scope).unwrap()[2] = 31;
    });
    assert_eq!(js_buffer_get(a, 4), 31);
    js_buffer_write_uint16_le(value(nested), 0x1234 as f64, 0);
    assert_eq!(js_buffer_get(source, 4), 0x34);
    assert_eq!(js_buffer_get(b, 1), 0x12);
    assert_eq!(buffer_byte_offset(nested as usize), 4);
    assert_eq!(
        buffer_backing_array_buffer(a as usize),
        buffer_backing_array_buffer(b as usize)
    );
    bytes::no_gc(|_| {
        assert_eq!(
            crate::native_abi::js_native_abi_check_buffer_data_ptr(value(nested)),
            bytes::span(value(nested), false).unwrap().ptr as *const u8
        )
    });
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
        assert_eq!(
            unsafe { super::store::length(view as usize) as u32 },
            length
        );
        assert_eq!(buffer_byte_offset(view as usize), offset);
        assert_eq!(buffer_backing_array_buffer(view as usize), backing);
        bytes::no_gc(|_| unsafe {
            assert_eq!(
                bytes::span(value(view), false).unwrap().ptr,
                bytes::span(value(source), false)
                    .unwrap()
                    .ptr
                    .add(offset as usize)
            );
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
    for _ in 0..1024 {
        let view = js_buffer_slice(source, 0, 1);
        header::finalize_collected_dead_buffer(view as usize);
    }
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
    assert_eq!(unsafe { super::store::length(source as usize) as u32 }, 0);
    assert_eq!(js_buffer_length(view), 0);
    assert_eq!(js_buffer_length(backing as *const BufferHeader), 0);
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

#[test]
fn changing_a_view_owner_changes_the_resolved_window() {
    let first = js_uint8array_alloc(16);
    let second = js_uint8array_alloc(16);
    js_buffer_set(first, 3, 57);
    js_buffer_set(second, 3, 91);
    let sub = js_buffer_slice(first, 3, 11);
    assert_eq!(js_buffer_get(sub, 0), 57);
    unsafe {
        (*sub).link = second as usize;
    }
    assert_eq!(js_buffer_get(sub, 0), 91);
    unsafe {
        (*sub).link = first as usize;
    }
    assert_eq!(js_buffer_get(sub, 0), 57);
}

#[test]
fn byte_view_admission_does_not_depend_on_thread_local_metadata() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = js_uint8array_alloc(32);
    let _owner = scope.root_raw_mut_ptr(owner);
    js_buffer_set(owner, 7, 42);
    let sub = js_buffer_slice(owner, 7, 15);
    let _sub = scope.root_raw_mut_ptr(sub);
    let address = sub as usize;
    let pin = bytes::pin(value(owner)).unwrap();
    let expected = unsafe { pin.as_ptr().add(7) } as usize;
    let owner = owner as usize;
    std::thread::spawn(move || {
        assert_eq!(view::lookup(address).unwrap().backing, owner as usize);
        assert_eq!(
            bytes::no_gc(
                |_| bytes::span(value(address as *const BufferHeader), false)
                    .unwrap()
                    .ptr as usize
            ),
            expected
        );
        assert_eq!(js_buffer_get(address as *const BufferHeader, 0), 42);
    })
    .join()
    .unwrap();
}
