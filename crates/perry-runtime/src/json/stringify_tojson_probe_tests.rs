use super::*;

fn markers() -> Vec<&'static [u8]> {
    let mut names = vec![
        b"toJSON".as_slice(),
        b"__module__",
        crate::object::FETCH_SUBCLASS_HANDLE_FIELD,
    ];
    #[cfg(feature = "temporal")]
    names.push(crate::object::TEMPORAL_SUBCLASS_CELL_FIELD);
    names
}

unsafe fn check(bytes: &[u8]) {
    let expected = markers().iter().any(|name| *name == bytes);
    let header = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let value = JSValue::string_ptr(header);
    assert_eq!(key_may_carry_to_json(value), expected, "{bytes:?}");
    assert_eq!(marker_bytes_may_carry_to_json(bytes), expected, "{bytes:?}");
    // Preserve the previous probe's contract for runtime-valid stored values.
    let previous = markers()
        .iter()
        .any(|name| crate::string::js_string_key_matches_bytes(value, name));
    assert_eq!(key_may_carry_to_json(value), previous, "{bytes:?}");
}

#[test]
fn json_tojson_key_probe_matches_markers_and_all_single_byte_changes() {
    unsafe {
        for marker in markers() {
            check(marker);
            for at in 0..marker.len() {
                let mut bytes = marker.to_vec();
                for byte in 0..=255u8 {
                    bytes[at] = byte;
                    check(&bytes);
                }
            }
            for len in 0..marker.len() {
                check(&marker[..len]);
            }
            for byte in [0, b'_', b't', b'x', 0x80, 0xff] {
                let mut bytes = marker.to_vec();
                bytes.push(byte);
                check(&bytes);
                let mut bytes = vec![byte];
                bytes.extend_from_slice(marker);
                check(&bytes);
            }
        }
        for bytes in [
            b"active".as_slice(),
            b"toJson",
            b"TOJSON",
            b"_module_",
            b"__module___",
            b"t",
            b"_",
            b"",
            b"\xed\xa0\x80",
            b"t\0JSON",
        ] {
            check(bytes);
        }
        check(&vec![b't'; 4097]);
        check(&vec![b'_'; 4097]);
    }
}

#[test]
fn json_tojson_key_probe_rejects_inline_and_nonstring_values_without_allocation() {
    unsafe {
        let before = crate::arena::arena_total_bytes();
        for len in 0..=crate::value::SHORT_STRING_MAX_LEN {
            let mut bytes = vec![b'_'; len];
            for at in 0..len {
                for byte in 0..=127u8 {
                    bytes[at] = byte;
                    if let Some(value) = JSValue::try_short_string(&bytes) {
                        assert!(!key_may_carry_to_json(value));
                    }
                }
            }
            assert!(!key_may_carry_to_json(
                JSValue::try_short_string(&vec![b't'; len]).unwrap()
            ));
        }
        for value in [
            JSValue::null(),
            JSValue::undefined(),
            JSValue::int32(42),
            JSValue::bool(true),
            JSValue::from_bits(crate::value::TAG_HOLE),
            JSValue::string_ptr(std::ptr::null_mut()),
        ] {
            assert!(!key_may_carry_to_json(value));
        }
        assert_eq!(crate::arena::arena_total_bytes(), before);
    }
}

#[test]
fn json_tojson_key_array_probe_observes_replacement_without_managed_scratch() {
    unsafe {
        let arr = crate::array::js_array_alloc_with_length(4);
        let scope = crate::gc::RuntimeHandleScope::new();
        let keys = scope.root_raw_mut_ptr(arr);
        let ordinary = JSValue::try_short_string(b"name").unwrap();
        for i in 0..4 {
            crate::array::js_array_set(keys.get_raw_mut_ptr(), i, ordinary);
        }
        assert!(!keys_array_may_carry_to_json(keys.get_raw_mut_ptr()));
        for marker in markers() {
            for at in 0..4 {
                let value = JSValue::string_ptr(crate::js_string_from_bytes(
                    marker.as_ptr(),
                    marker.len() as u32,
                ));
                crate::array::js_array_set(keys.get_raw_mut_ptr(), at, value);
                let before = crate::arena::arena_total_bytes();
                let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
                for _ in 0..100 {
                    assert!(keys_array_may_carry_to_json(keys.get_raw_mut_ptr()));
                }
                assert_eq!(crate::arena::arena_total_bytes(), before);
                assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
                crate::array::js_array_set(keys.get_raw_mut_ptr(), at, ordinary);
                assert!(!keys_array_may_carry_to_json(keys.get_raw_mut_ptr()));
            }
        }
        let arr = keys.get_raw_mut_ptr::<crate::ArrayHeader>();
        let len = (*arr).length;
        (*arr).length = (*arr).capacity + 1;
        assert!(keys_array_may_carry_to_json(arr));
        (*arr).length = len;
        assert!(keys_array_may_carry_to_json((arr as *mut u8).add(1).cast()));
        assert!(keys_array_may_carry_to_json(std::ptr::null_mut()));
    }
}

#[cfg(unix)]
#[test]
fn json_tojson_marker_comparisons_stop_before_guard_page() {
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let raw = libc::mmap(
            std::ptr::null_mut(),
            page * 2,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANON | libc::MAP_PRIVATE,
            -1,
            0,
        );
        assert_ne!(raw, libc::MAP_FAILED);
        let base = raw.cast::<u8>();
        assert_eq!(
            libc::mprotect(base.add(page).cast(), page, libc::PROT_NONE),
            0
        );
        for marker in markers() {
            for len in 0..=marker.len() + 1 {
                let start = base.add(page - len);
                std::ptr::write_bytes(start, b'_', len);
                if len == marker.len() {
                    std::ptr::copy_nonoverlapping(marker.as_ptr(), start, len);
                }
                let bytes = std::slice::from_raw_parts(start, len);
                assert_eq!(
                    marker_bytes_may_carry_to_json(bytes),
                    markers().iter().any(|name| *name == bytes)
                );
            }
        }
        assert_eq!(libc::munmap(raw, page * 2), 0);
    }
}
