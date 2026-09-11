use super::*;
fn oracle(bytes: &[u8]) -> Option<usize> {
    bytes.iter().enumerate().position(|(i, &b)| {
        b == b'"'
            || b == 0x5c
            || b < 32
            || (b == 0xed && bytes.get(i + 1).is_some_and(|n| n & 0xe0 == 0xa0))
    })
}
#[test]
fn json_surrogate_scan_matches_scalar_at_all_boundaries() {
    for len in [
        0, 1, 2, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65, 79, 80, 81, 127, 128, 129, 255, 256,
        257,
    ] {
        for align in [0, 1, 7, 15] {
            let mut storage = vec![b'a'; align + len + 16];
            storage[align + len..].fill(b'"');
            for i in 0..len {
                for byte in 0..=255u8 {
                    storage[align + i] = byte;
                    let b = &storage[align..align + len];
                    assert_eq!(scan(b), oracle(b));
                    assert_eq!(scalar(b), oracle(b));
                }
                storage[align + i] = b'a';
            }
            for i in 0..len.saturating_sub(1) {
                storage[align + i] = 0xed;
                for byte in 0..=255u8 {
                    storage[align + i + 1] = byte;
                    let b = &storage[align..align + len];
                    assert_eq!(scan(b), oracle(b));
                    assert_eq!(scalar(b), oracle(b));
                }
                storage[align + i] = b'a';
                storage[align + i + 1] = b'a';
            }
        }
    }
}
#[cfg(unix)]
#[test]
fn json_surrogate_scan_stops_at_guard_page() {
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let allocation = libc::mmap(
            std::ptr::null_mut(),
            page * 2,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANON | libc::MAP_PRIVATE,
            -1,
            0,
        );
        assert_ne!(allocation, libc::MAP_FAILED);
        let base = allocation.cast::<u8>();
        assert_eq!(
            libc::mprotect(base.add(page).cast(), page, libc::PROT_NONE),
            0
        );
        for len in 0..=257 {
            let start = base.add(page - len);
            std::ptr::write_bytes(start, b'a', len);
            for distance in 0..len.min(4) {
                // GC_STORE_AUDIT(POINTER_FREE): isolated byte-buffer scanner fixture.
                start.add(len - distance - 1).write(0xed);
                let bytes = std::slice::from_raw_parts(start, len);
                assert_eq!(scan(bytes), oracle(bytes));
                if distance != 0 {
                    // GC_STORE_AUDIT(POINTER_FREE): isolated byte-buffer scanner fixture.
                    start.add(len - distance).write(0xa0);
                    let bytes = std::slice::from_raw_parts(start, len);
                    assert_eq!(scan(bytes), oracle(bytes));
                    // GC_STORE_AUDIT(POINTER_FREE): isolated byte-buffer scanner fixture.
                    start.add(len - distance).write(b'a');
                }
                // GC_STORE_AUDIT(POINTER_FREE): isolated byte-buffer scanner fixture.
                start.add(len - distance - 1).write(b'a');
            }
            let bytes = std::slice::from_raw_parts(start, len);
            assert_eq!(scan(bytes), oracle(bytes));
        }
        assert_eq!(libc::munmap(allocation, page * 2), 0);
    }
}

#[test]
fn json_surrogate_scan_routes_tokens_to_the_normalizing_builder() {
    use crate::json::parser::{DirectParser, ParsedStr};
    for prefix in [0, 13, 14, 15, 16, 61, 62, 63, 64, 127, 128, 255, 256] {
        for (payload, owned) in [
            (&[0xed, 0xa0, 0x80][..], true),
            (&[0xed, 0xb0, 0x80][..], true),
            ("한🙂".as_bytes(), false),
        ] {
            let mut token = vec![b'"'];
            token.extend(std::iter::repeat_n(b'a', prefix));
            token.extend_from_slice(payload);
            token.push(b'"');
            let mut parser = DirectParser::new(&token);
            let parsed = parser.parse_string_bytes().expect("valid JSON token");
            assert_eq!(matches!(parsed, ParsedStr::Owned(_)), owned);
            assert_eq!(parsed.as_bytes(), &token[1..token.len() - 1]);
        }
    }
}
