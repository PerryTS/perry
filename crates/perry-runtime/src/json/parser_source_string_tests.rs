use super::*;

fn count(bytes: &[u8]) -> u32 {
    crate::string::compute_utf16_len(bytes.as_ptr(), bytes.len() as u32)
}

#[test]
fn json_source_count_preserves_all_two_byte_and_malformed_tail_interpretations() {
    let mut admitted = 0;
    let mut declined = 0;
    for first in 0..=255u8 {
        for second in 0..=255u8 {
            let payload = [first, second];
            // The longer prefix also exercises the vector/validated counter
            // for the source while the token uses the bounded scalar counter.
            for prefix_len in [1, 63, 64, 65] {
                let mut input = vec![b' '; prefix_len];
                input.extend_from_slice(&payload);
                input.extend_from_slice(b"\"}");
                match count_from_ascii_surroundings(
                    &input,
                    prefix_len,
                    prefix_len + payload.len(),
                    count(&input),
                ) {
                    Some(units) => {
                        assert_eq!(units, count(&payload), "{payload:?} at {prefix_len}");
                        admitted += 1;
                    }
                    None => declined += 1,
                }
            }
        }
    }
    assert!(admitted > 100_000 && declined > 10_000);
}

#[test]
fn json_source_count_checks_astral_surrogate_and_cross_boundary_tails() {
    for payload in [
        "ASCII é中😀".as_bytes(),
        b"\xed\xa0\x80\xed\xb0\x80", // raw WTF-8 surrogate pair
        b"\xf4\x90\x80\x80x",        // out-of-range point, bounded legacy count
        b"\xc0\x80\x80x",            // overlong plus stray continuation
        b"\xf0abcx",                 // malformed lead skipping ASCII
        b"\xf0\x80\x80",             // truncated four-byte tail
        b"\xe0\x80",                 // truncated three-byte tail
        b"x\xc0",                    // truncated two-byte tail
    ] {
        for padding in 0..128 {
            let mut input = b"{\"text\":\"".to_vec();
            let start = input.len();
            input.resize(start + padding, b'x');
            input.extend_from_slice(payload);
            let end = input.len();
            input.extend_from_slice(b"\"}");
            if let Some(units) = count_from_ascii_surroundings(&input, start, end, count(&input)) {
                assert_eq!(units, count(&input[start..end]));
            }
        }
    }
    assert!(count_from_ascii_surroundings(b"\"abc\"", 1, 4, 1).is_none());
    assert!(count_from_ascii_surroundings(b"\"abc\"", 1, 4, 100).is_none());
    assert!(count_from_ascii_surroundings(b"\"abc\"", 4, 1, 5).is_none());
}

#[test]
fn json_source_count_requires_the_exact_owner_and_ascii_surroundings() {
    let payload = "é中😀".repeat(100_000);
    for (prefix, suffix, expected) in [
        ("{\"text\":\"", "\"}", true),
        (" \"", "\" \n", true),
        ("{\"é\":\"", "\"}", false),
        ("{\"text\":\"", "\",\"é\":0}", false),
    ] {
        let input = format!("{prefix}{payload}{suffix}");
        let scope = crate::gc::RuntimeHandleScope::new();
        let source = scope.root_string_ptr(crate::js_string_from_bytes(
            input.as_ptr(),
            input.len() as u32,
        ));
        source.with_const_ptr(|source: *const crate::StringHeader| unsafe {
            let _suppress = crate::gc::GcSuppressScope::new();
            let bytes = std::slice::from_raw_parts(crate::string::string_data(source), input.len());
            let end = prefix.len() + payload.len();
            let token = &bytes[prefix.len()..end];
            let mut parser = DirectParser::new_batched_from_string(bytes, source);
            parser.pos = end + 1;
            assert_eq!(
                parser.borrowed_source_utf16_len(token),
                expected.then_some(400_000)
            );
            assert!(parser
                .borrowed_source_utf16_len(payload.as_bytes())
                .is_none());
            // An equal-sized independent byte buffer is not this source.
            parser.input = input.as_bytes();
            assert!(parser
                .borrowed_source_utf16_len(&parser.input[prefix.len()..end])
                .is_none());
            parser.input = &bytes[1..];
            parser.pos -= 1;
            assert!(parser.borrowed_source_utf16_len(token).is_none());
            parser.source = std::ptr::null();
            assert!(parser.borrowed_source_utf16_len(token).is_none());
        });
    }
    let mut input = vec![b' '; MAX_SURROUNDING_BYTES];
    input.extend_from_slice(b"\"x\"");
    assert!(
        count_from_ascii_surroundings(&input, input.len() - 2, input.len() - 1, count(&input))
            .is_none()
    );
}
