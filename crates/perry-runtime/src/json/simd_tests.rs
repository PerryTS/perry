use super::*;

#[test]
fn digit_scan_matches_scalar_at_every_byte_and_boundary() {
    let check_digits = |bytes: &[u8]| {
        assert_eq!(
            count_ascii_digits(bytes),
            bytes.iter().take_while(|b| b.is_ascii_digit()).count(),
            "{bytes:?}"
        );
    };
    for len in [0, 1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
        for alignment in [0, 1, 7, 15] {
            let mut storage = vec![b'5'; alignment + len + 16];
            storage[alignment + len..].fill(b'!');
            check_digits(&storage[alignment..alignment + len]);
            for i in 0..len {
                for byte in 0..=255u8 {
                    storage[alignment + i] = byte;
                    check_digits(&storage[alignment..alignment + len]);
                }
                storage[alignment + i] = b'5';
            }
        }
    }
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            check_digits(&[a, b, b'0', b'9', b'1', b'8', b'0', b'9']);
            check_digits(&[b'0', b'9', b'1', b'8', b'0', b'9', a, b]);
        }
    }
}

#[cfg(unix)]
#[test]
fn digit_and_string_scans_stop_before_guard_page() {
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
        for len in [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129,
        ] {
            let start = base.add(page - len);
            std::ptr::write_bytes(start, b'5', len);
            let bytes = std::slice::from_raw_parts(start, len);
            assert_eq!(count_ascii_digits(bytes), len);
            assert_eq!(find_string_terminator(bytes), None);
            if len != 0 {
                start.add(len - 1).write(b'"');
                let bytes = std::slice::from_raw_parts(start, len);
                assert_eq!(count_ascii_digits(bytes), len - 1);
                assert_eq!(find_string_terminator(bytes), Some(len - 1));
            }
        }
        assert_eq!(libc::munmap(raw, page * 2), 0);
    }
}

fn check(bytes: &[u8]) {
    // The oracles describe the three contracts independently of the bit tricks.
    let parse = bytes
        .iter()
        .position(|&b| b == b'"' || b == b'\\' || b < 32);
    let escape = bytes
        .iter()
        .position(|&b| b == b'"' || b == b'\\' || b < 32 || b == 237);
    let quotes = bytes.iter().position(|&b| b == b'"' || b == b'\\');
    assert_eq!(find_string_terminator(bytes), parse, "parse {bytes:?}");
    assert_eq!(find_string_escape(bytes), escape, "escape {bytes:?}");
    assert_eq!(find_quote_or_backslash(bytes), quotes, "quotes {bytes:?}");
    assert_eq!(
        find_word::<true, false>(bytes),
        parse,
        "word parse {bytes:?}"
    );
    assert_eq!(
        find_word::<true, true>(bytes),
        escape,
        "word escape {bytes:?}"
    );
    assert_eq!(
        find_word::<false, false>(bytes),
        quotes,
        "word quotes {bytes:?}"
    );
}

#[test]
fn every_byte_at_vector_word_and_tail_boundaries() {
    for len in [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 23, 24, 31, 32, 33, 63, 64, 65,
    ] {
        for alignment in [0, 1, 7, 15] {
            let mut storage = vec![b'a'; alignment + len + 16];
            // A scanner must ignore even special bytes immediately past the slice.
            storage[alignment + len..].fill(b'"');
            check(&storage[alignment..alignment + len]);
            for index in 0..len {
                for byte in 0..=255u8 {
                    storage[alignment + index] = byte;
                    check(&storage[alignment..alignment + len]);
                }
                storage[alignment + index] = b'a';
            }
        }
    }
}

#[test]
fn adjacent_byte_borrows_and_unsigned_controls() {
    for left in 0..=255u8 {
        for right in 0..=255u8 {
            check(&[left, right, 32, 33, 0x80, 0xFF, b'a', b'b']);
            check(&[b'a', b'b', 0x80, 0xFF, 32, 33, left, right]);
            for len in 4..8 {
                check(&[left, right, 32, 33, 0x80, 0xFF, b'a'][..len]);
                let mut tail = [b'a'; 7];
                tail[len - 2] = left;
                tail[len - 1] = right;
                check(&tail[..len]);
            }
        }
    }
}

#[test]
fn random_byte_strings_match_scalar_contracts() {
    let mut state = 0x92D6_482F_8017_AECBu64;
    for n in 0..12000 {
        let mut storage = vec![0; 19 + n % 257];
        for byte in &mut storage {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        check(&storage[n % 19..]);
    }
}
