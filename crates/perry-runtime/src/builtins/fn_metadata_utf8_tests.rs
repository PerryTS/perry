use super::decode_registered_bytes;

#[test]
fn metadata_decoding_matches_checked_utf8_at_offsets_and_word_boundaries() {
    // Exercise every byte value in every slot of short inputs, across all
    // alignments and both sides of a machine-word boundary. This validates the
    // unsafe ASCII conversion against the checked contract, not a header hint.
    for offset in 0..16 {
        for len in 0..=4 * std::mem::size_of::<usize>() + 1 {
            let mut storage = vec![b'a'; offset + len + 16];
            let range = offset..offset + len;
            assert_eq!(
                decode_registered_bytes(&storage[range.clone()]),
                std::str::from_utf8(&storage[range.clone()]).ok()
            );
            for position in 0..len {
                for byte in 0..=255u8 {
                    storage[offset + position] = byte;
                    let bytes = &storage[range.clone()];
                    assert_eq!(
                        decode_registered_bytes(bytes),
                        std::str::from_utf8(bytes).ok(),
                        "offset={offset} len={len} position={position} byte={byte}"
                    );
                }
                storage[offset + position] = b'a';
            }
        }
    }
    for bytes in [
        "recovered世界".as_bytes(),
        "é".as_bytes(),
        "🦀".as_bytes(),
        &[0xc0, 0xaf],
        &[0xed, 0xa0, 0x80],
        &[0xf0, 0x9f],
        &[0xff],
        b"ASCII\0name",
    ] {
        assert_eq!(
            decode_registered_bytes(bytes),
            std::str::from_utf8(bytes).ok()
        );
    }
}
#[cfg(unix)]
#[test]
fn metadata_ascii_covering_reads_stay_between_guard_pages() {
    struct Mapping {
        base: *mut libc::c_void,
        len: usize,
    }
    impl Drop for Mapping {
        fn drop(&mut self) {
            unsafe { libc::munmap(self.base, self.len) };
        }
    }
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        assert!(page >= 4096);
        let base = libc::mmap(
            std::ptr::null_mut(),
            page * 3,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(base, libc::MAP_FAILED);
        let _mapping = Mapping {
            base,
            len: page * 3,
        };
        assert_eq!(libc::mprotect(base, page, libc::PROT_NONE), 0);
        let middle = base.cast::<u8>().add(page);
        assert_eq!(
            libc::mprotect(middle.add(page).cast(), page, libc::PROT_NONE),
            0
        );
        middle.write_bytes(b'a', page);
        for len in 0..=4 * std::mem::size_of::<usize>() + 1 {
            for start in [middle, middle.add(page - len)] {
                let bytes = std::slice::from_raw_parts(start, len);
                assert_eq!(
                    decode_registered_bytes(bytes),
                    std::str::from_utf8(bytes).ok()
                );
                if len != 0 {
                    start.add(len - 1).write(0xff);
                    let bytes = std::slice::from_raw_parts(start, len);
                    assert_eq!(decode_registered_bytes(bytes), None);
                    start.add(len - 1).write(b'a');
                }
            }
        }
        // A zero-length slice must not read even its first address.
        let empty = std::slice::from_raw_parts(base.cast::<u8>(), 0);
        assert_eq!(decode_registered_bytes(empty), Some(""));
    }
}
