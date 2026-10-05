use super::*;

fn value(buf: *const BufferHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
}

#[test]
fn suffix_views_allocate_one_pointer_and_share_native_bytes() {
    let source = js_uint8array_alloc(4096);
    let scope = crate::gc::RuntimeHandleScope::new();
    let _source = scope.root_raw_mut_ptr(source);
    for start in 0..4096 {
        let view = js_buffer_slice(source, start, 4096);
        unsafe {
            assert_eq!((*view).length, 4096 - start as u32);
            assert_eq!((*view).capacity, std::mem::size_of::<usize>() as u32);
            let header = crate::value::addr_class::try_read_gc_header(view as usize).unwrap();
            assert_eq!(
                header.size as usize,
                crate::gc::GC_HEADER_SIZE + 8 + std::mem::size_of::<usize>()
            );
            assert_ne!(header._reserved & crate::gc::GC_BUFFER_VIEW_DATA, 0);
            assert_eq!(buffer_data(view), buffer_data(source).add(start as usize));
        }
        assert!(is_uint8array_buffer(view as usize));
    }
}

#[test]
fn byte_views_resolve_once_and_never_enter_the_owning_cache() {
    let owner = js_uint8array_alloc(32);
    let sub = js_buffer_slice(owner, 4, 20);
    let nested = js_buffer_slice(sub, 3, 9);
    js_buffer_set(owner, 7, 91);
    let expected = unsafe { buffer_data(owner).add(7) };
    let before = view::lookup_count();
    for _ in 0..128 {
        assert_eq!(js_buffer_get(nested, 0), 91);
        assert_eq!(js_buffer_index_get_value(nested, 0), 91.0);
        assert_eq!(
            js_buffer_index_get_value(nested, 6).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(buffer_data(nested), expected);
    }
    let after = view::lookup_count();
    assert_eq!(
        after - before,
        0,
        "byte reads must not look up view metadata"
    );
    header::u8_inline_cache_try_prime(nested as usize);
    assert!(!header::test_u8_inline_cache_holds(nested as usize));
}

#[test]
fn rewritten_view_backing_refreshes_the_derived_pointer() {
    let owner = js_uint8array_alloc(32);
    let replacement = js_uint8array_alloc(32);
    js_buffer_set(owner, 5, 17);
    js_buffer_set(replacement, 5, 29);
    let sub = js_buffer_slice(owner, 5, 15);
    assert_eq!(js_buffer_get(sub, 0), 17);
    view::visit_backing_slot(sub as usize, |slot| unsafe {
        *slot = replacement as u64;
    });
    assert_eq!(js_buffer_get(sub, 0), 29);
    assert_eq!(buffer_data(sub), unsafe { buffer_data(replacement).add(5) });
    view::visit_backing_slot(sub as usize, |slot| unsafe {
        *slot = owner as u64;
    });
}

#[test]
fn data_view_caches_its_stable_window_pointer_in_the_header_payload() {
    let backing = js_array_buffer_new(16);
    let backing_value = value(backing);
    let view_value = js_data_view_new(backing_value, 4.0, 8.0);
    let view = crate::value::JSValue::from_bits(view_value.to_bits()).as_pointer::<BufferHeader>()
        as *mut BufferHeader;

    unsafe {
        assert_eq!((*view).length, 8);
        assert_eq!((*view).capacity, std::mem::size_of::<usize>() as u32);
        assert_eq!(
            view::data_view_data_ptr(view) as *const u8,
            buffer_data(backing).add(4)
        );
    }

    js_data_view_set(
        view_value,
        0.0,
        0x0102_0304 as f64,
        DataViewKind::Uint32,
        false,
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(buffer_data(backing).add(4), 4) },
        &[1, 2, 3, 4]
    );
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

#[test]
fn hot_read_probe_detects_disabled_view_pointer_layout() {
    let owner = js_uint8array_alloc(16);
    js_buffer_set(owner, 3, 57);
    let sub = js_buffer_slice(owner, 3, 11);
    let gc = unsafe { crate::gc::header_from_trusted_user_ptr(sub.cast()).cast_mut() };
    let original = unsafe { (*gc)._reserved };
    let probe = || {
        let before = view::lookup_count();
        for _ in 0..16 {
            assert_eq!(js_buffer_get(sub, 0), 57);
            let _ = buffer_data(sub);
        }
        assert_eq!(
            view::lookup_count() - before,
            0,
            "hot byte reads consulted view metadata"
        );
    };
    probe();
    // Sabotage the representation proof. Reads still return the correct byte,
    // but re-enter the old registry path; the hot-path acceptance probe MUST
    // reject that state, then accept the restored layout again.
    unsafe { (*gc)._reserved &= !crate::gc::GC_BUFFER_VIEW_DATA };
    let detected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(probe));
    unsafe { (*gc)._reserved = original };
    assert!(
        detected.is_err(),
        "the hot-read probe could not detect sabotage"
    );
    probe();
}

#[test]
fn entry_byte_proof_resolves_both_layouts_and_rejects_other_receivers() {
    let owner = js_uint8array_alloc(32);
    let sub = js_buffer_slice(owner, 7, 15);
    let resolve = header::js_u8_resolve_read_data;
    assert_eq!(resolve(value(owner)), buffer_data(owner) as usize);
    assert_eq!(resolve(value(sub)), unsafe { buffer_data(owner).add(7) } as usize);
    assert!(!header::test_u8_inline_cache_holds(sub as usize));
    assert_eq!(resolve(17.0), 0);
    assert_eq!(resolve(f64::from_bits(crate::value::TAG_UNDEFINED)), 0);
    let raw_buffer = js_buffer_alloc(8, 0);
    mark_as_array_buffer(raw_buffer as usize);
    assert_eq!(resolve(value(raw_buffer)), 0);
    let foreign = header::buffer_alloc_foreign(buffer_data(owner) as *mut u8, 32);
    mark_as_uint8array(foreign as usize);
    assert_eq!(resolve(value(foreign)), 0);
    let pointer = resolve(value(owner));
    detach_array_buffer(buffer_backing_array_buffer(owner as usize));
    assert_eq!(resolve(value(owner)), pointer);
    assert_eq!(unsafe { (*owner).length }, 0);
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
    let expected = unsafe { buffer_data(owner).add(7) } as usize;
    std::thread::spawn(move || {
        assert!(view::lookup(address).is_none());
        header::u8_inline_cache_try_prime(address);
        assert!(!header::test_u8_inline_cache_holds(address));
        assert_eq!(
            buffer_data(address as *const BufferHeader) as usize,
            expected
        );
        assert_eq!(js_buffer_get(address as *const BufferHeader, 0), 42);
    })
    .join()
    .unwrap();
}
