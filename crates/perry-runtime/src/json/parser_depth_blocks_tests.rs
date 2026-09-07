fn check(bytes: &[u8]) {
    for limit in [0, 1, 2, 8, 1024] {
        assert_eq!(
            super::nesting_depth_exceeds(bytes, limit),
            super::scan_tests::reference(bytes, limit),
            "limit={limit}, bytes={bytes:?}"
        );
    }
}

#[test]
fn bracket_quote_and_invalid_escape_combinations_cross_block_boundaries() {
    let alphabet = b"[]\"\\a";
    // All six-byte patterns, repeated across several windows. Moving the
    // prefix puts every transition at every vector position, including a
    // backslash outside a string that must not escape a following quote.
    for pattern in 0..15625usize {
        let mut n = pattern;
        let mut part = [0u8; 6];
        for b in &mut part {
            *b = alphabet[n % 5];
            n /= 5;
        }
        for prefix in 0..17 {
            let mut bytes = vec![b'a'; prefix];
            for _ in 0..8 {
                bytes.extend_from_slice(&part);
            }
            bytes.extend_from_slice(b"[[[]]]\"}]");
            check(&bytes);
        }
    }
}

#[test]
#[cfg(unix)]
fn structural_windows_and_scalar_tails_stop_before_a_guard_page() {
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
        for len in 0..1024 {
            for suffix in [b"".as_slice(), b"\"[[[[]]]]", b"x\"[[[[]]]]", b"\x00\"[]"] {
                for fill in [b'a', b'[', b']', b'\\', b'"', 0xff] {
                    let mut bytes = vec![fill; len];
                    bytes.extend_from_slice(suffix);
                    let at = base.add(page - bytes.len());
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
                    check(std::slice::from_raw_parts(at, bytes.len()));
                }
            }
        }
        assert_eq!(libc::munmap(raw, page * 2), 0);
    }
}
