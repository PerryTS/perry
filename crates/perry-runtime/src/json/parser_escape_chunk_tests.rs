use super::*;

fn decoded(source: &[u8]) -> Option<Vec<u8>> {
    let mut parser = DirectParser::new(source);
    let result = parser.parse_string_bytes().map(|v| v.as_bytes().to_vec());
    assert_eq!(result.is_some(), parser.valid);
    if result.is_some() {
        assert_eq!(parser.pos, source.len());
    }
    result
}

#[test]
fn escaped_strings_cross_chunk_boundaries_with_utf16_surrogates() {
    // Expected bytes are independent of the parser. Move each escape across
    // the scalar/chunk boundary, successive chunks, and the final scalar tail.
    let forms: &[(&[u8], &[u8])] = &[
        (br#"\"\\\/\b\f\n\r\t"#, b"\"\\/\x08\x0c\n\r\t"),
        (
            br#"\u0000\u007f\u0080\u07ff\u0800"#,
            "\0\u{7f}\u{80}\u{7ff}\u{800}".as_bytes(),
        ),
        (
            br#"\uD800\uDC00\uDBFF\uDFFF"#,
            "\u{10000}\u{10ffff}".as_bytes(),
        ),
        (
            br#"\uD800x\uDC00\uD800\u0061"#,
            b"\xed\xa0\x80x\xed\xb0\x80\xed\xa0\x80a",
        ),
        (
            br#"\uD800\uD800\uDC00\uDC00"#,
            b"\xed\xa0\x80\xf0\x90\x80\x80\xed\xb0\x80",
        ),
    ];
    for prefix in 0..160 {
        for &(escaped, expected) in forms {
            let mut source = br#""\n"#.to_vec();
            let mut wanted = b"\n".to_vec();
            source.extend(std::iter::repeat_n(b'a', prefix));
            wanted.extend(std::iter::repeat_n(b'a', prefix));
            for _ in 0..8 {
                source.extend_from_slice(escaped);
                wanted.extend_from_slice(expected);
                source.extend_from_slice("é中🙂".as_bytes());
                wanted.extend_from_slice("é中🙂".as_bytes());
            }
            source.push(b'"');
            assert_eq!(decoded(&source), Some(wanted));
            for end in 0..source.len() {
                assert!(
                    decoded(&source[..end]).is_none(),
                    "prefix={prefix}, end={end}"
                );
            }
        }
    }
}

#[test]
fn escaped_strings_match_serde_and_reject_invalid_chunk_contents() {
    let alphabet = [
        'a', '\0', '\u{8}', '\t', '\n', '\u{c}', '\r', '"', '\\', 'é', '中', '🙂',
    ];
    let mut state = 0xf120_9365_29b7_c80du64;
    for length in 0..1024 {
        let value: String = (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                alphabet[state as usize % alphabet.len()]
            })
            .collect();
        let json = serde_json::to_vec(&value).unwrap();
        assert_eq!(decoded(&json).as_deref(), Some(value.as_bytes()));
    }
    for at in 0..256 {
        for bad in [b"\x00".as_slice(), b"\x1f", br"\q", br"\u!000", br"\u123z"] {
            let mut source = br#""\n"#.to_vec();
            source.extend(std::iter::repeat_n(b'a', at));
            source.extend_from_slice(bad);
            source.extend(std::iter::repeat_n(b'a', 128));
            source.push(b'"');
            assert!(decoded(&source).is_none(), "at={at}, bad={bad:?}");
        }
    }
}

#[test]
fn escaped_chunks_do_not_grow_scratch_before_the_string_needs_it() {
    for power in 6..19 {
        for delta in -8isize..=8 {
            let length = ((1usize << power) as isize + delta) as usize;
            // Start with an escape so the initial copied prefix is empty.
            // A final quote within the next window must not force a reserve.
            let mut source = br#""\n"#.to_vec();
            source.extend(std::iter::repeat_n(b'a', length - 1));
            source.push(b'"');
            let mut parser = DirectParser::new(&source);
            let Some(ParsedStr::Owned(bytes)) = parser.parse_string_bytes() else {
                panic!("escaped string must own its decoded bytes");
            };
            assert_eq!(bytes.len(), length);
            assert_eq!(bytes.capacity(), length.next_power_of_two());
            assert_eq!(bytes[0], b'\n');
            assert!(bytes[1..].iter().all(|&byte| byte == b'a'));
            assert_eq!(parser.pos, source.len());
        }
    }
}

#[test]
#[cfg(unix)]
fn escaped_chunk_never_reads_past_guarded_input() {
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
        let pattern = br#"\n\uD800\uDC00abc\"\\\uDC00"#;
        for length in 1..1024 {
            let mut source = vec![b'"'];
            source.extend(pattern.iter().copied().cycle().take(length - 1));
            let input = base.add(page - length);
            std::ptr::copy_nonoverlapping(source.as_ptr(), input, length);
            assert!(decoded(std::slice::from_raw_parts(input, length)).is_none());
        }
        assert_eq!(libc::munmap(raw, page * 2), 0);
    }
}
