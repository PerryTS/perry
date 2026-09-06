use super::count;
use crate::string::{compute_utf16_len, compute_utf16_len_wtf8};

fn check(s: &str) {
    let expected = s.encode_utf16().count();
    assert_eq!(count(s), expected);
    assert_eq!(
        compute_utf16_len(s.as_ptr(), s.len() as u32) as usize,
        expected
    );
}

#[test]
fn all_unicode_scalars_and_mixed_blocks() {
    // All widths, including the surrogate gap and the maximum code point.
    let all: String = (0..=0x10ffff).filter_map(char::from_u32).collect();
    check(&all);
    for ch in [
        '\0',
        'a',
        '\u{7f}',
        '\u{80}',
        '\u{7ff}',
        '\u{800}',
        '\u{d7ff}',
        '\u{e000}',
        '\u{ffff}',
        '\u{10000}',
        '\u{10ffff}',
    ] {
        check(&ch.to_string().repeat(1024));
    }
    check(&"aé中🙂\0".repeat(4096));
}

#[test]
fn unaligned_substrings_and_every_vector_tail() {
    let storage = format!("{}{}", "a".repeat(64), "é中🙂a\0".repeat(64));
    for start in 0..64 {
        for end in start..storage.len() {
            if storage.is_char_boundary(end) {
                check(&storage[start..end]);
            }
        }
    }
}

#[test]
fn raw_bytes_keep_the_existing_wtf8_count() {
    assert_eq!(compute_utf16_len(std::ptr::null(), 0), 0);
    for prefix in 0..128 {
        for tail in [
            &[0xed, 0xa0, 0x80][..],
            &[0xed, 0xbf, 0xbf],
            &[0x80],
            &[0xc3],
            &[0xe2, 0x80],
            &[0xf0, 0x9f, 0x98],
            &[0xff, b'a', 0xfe],
            &[0xc0, 0x80],
        ] {
            let mut bytes = "é🙂".repeat(prefix).into_bytes();
            bytes.extend_from_slice(tail);
            assert!(std::str::from_utf8(&bytes).is_err());
            assert_eq!(
                compute_utf16_len(bytes.as_ptr(), bytes.len() as u32),
                compute_utf16_len_wtf8(&bytes)
            );
        }
    }
}

#[test]
fn byte_mutations_match_standard_validation_and_legacy_counting() {
    let text = "aé中🙂\0".repeat(12);
    let mut bytes = text.as_bytes().to_vec();
    for position in 0..bytes.len() {
        let original = bytes[position];
        for byte in 0..=255 {
            bytes[position] = byte;
            let expected = match std::str::from_utf8(&bytes) {
                Ok(s) => s.encode_utf16().count() as u32,
                Err(_) => compute_utf16_len_wtf8(&bytes),
            };
            assert_eq!(super::count_bytes(&bytes), expected);
        }
        bytes[position] = original;
    }
}

#[cfg(unix)]
#[test]
fn vector_loads_stay_inside_guarded_input() {
    struct Pages(*mut u8, usize);
    impl Drop for Pages {
        fn drop(&mut self) {
            unsafe {
                libc::munmap(self.0.cast(), self.1);
            }
        }
    }
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let base = libc::mmap(
            std::ptr::null_mut(),
            page * 3,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );
        assert_ne!(base, libc::MAP_FAILED);
        let pages = Pages(base.cast(), page * 3);
        let writable = pages.0.add(page);
        assert_eq!(
            libc::mprotect(writable.cast(), page, libc::PROT_READ | libc::PROT_WRITE),
            0
        );
        for len in 0..=512 {
            // Vary every byte alignment and tail size, with valid multibyte
            // sequences crossing SIMD lanes. Check both allocation edges.
            let s = format!("{}{}", "a".repeat(len % 10), "é中🙂a".repeat(len / 10));
            assert_eq!(s.len(), len);
            for data in [writable, writable.add(page - len)] {
                std::ptr::copy_nonoverlapping(s.as_ptr(), data, len);
                let bytes = std::slice::from_raw_parts(data, len);
                check(std::str::from_utf8(bytes).unwrap());
            }
        }
    }
}
