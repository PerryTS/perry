use super::source_token_utf16_len;
use crate::string::{compute_utf16_len, compute_utf16_len_wtf8};

fn check(payload: &[u8], prefix: &[u8], suffix: &[u8]) -> Option<u32> {
    let input = [prefix, payload, suffix].concat();
    let source_units = compute_utf16_len(input.as_ptr(), input.len() as u32);
    let answer = source_token_utf16_len(&input, source_units, prefix.len(), payload.len());
    if let Some(units) = answer {
        // Independent pre-existing scalar interpretation, including malformed
        // byte input. No valid-UTF-8 assumption is made by the metadata helper.
        assert_eq!(
            units,
            compute_utf16_len_wtf8(payload),
            "payload={payload:?}"
        );
    }
    answer
}

#[test]
fn json_source_lengths_admit_complete_unicode_and_ascii_surroundings() {
    for tail in ["a", "é", "東京", "🙂", "\u{10ffff}"] {
        let payload = format!("{}{tail}", "Grüße東京🙂".repeat(128));
        for (prefix, suffix) in [
            (&b"\""[..], &b"\""[..]),
            (&b" \n\t\""[..], &b"\"\r\n "[..]),
            (&b"{\"id\":17,\"text\":\""[..], &b"\",\"active\":true}"[..]),
        ] {
            assert_eq!(
                check(payload.as_bytes(), prefix, suffix),
                Some(payload.encode_utf16().count() as u32)
            );
        }
    }
}

#[test]
fn json_source_lengths_reject_ambiguous_tails_and_nonascii_surroundings() {
    for tail in [
        &b"\xc3"[..],
        &b"\xe2\x80"[..],
        &b"\xf0\x9f\x98"[..],
        &b"\xffab"[..],
    ] {
        let payload = [b"x".repeat(256), tail.to_vec()].concat();
        assert_eq!(check(&payload, b"\"", b"\""), None);
    }
    let payload = "東京🙂".repeat(128);
    assert_eq!(
        check(payload.as_bytes(), "{\"é\":\"".as_bytes(), b"\"}"),
        None
    );
    assert_eq!(
        check(
            payload.as_bytes(),
            b"{\"text\":\"",
            "\",\"é\":0}".as_bytes()
        ),
        None
    );
    assert_eq!(check(b"a", b"\"", b"\""), None);
    assert_eq!(source_token_utf16_len(b"abc", 3, usize::MAX, 2), None);
    assert_eq!(source_token_utf16_len(b"abc", 3, 2, 2), None);
    assert_eq!(source_token_utf16_len(&[b'a'; 32], 0, 1, 30), None);
}

#[test]
fn json_source_lengths_preserve_arbitrary_byte_interpretation() {
    let mut admitted = 0;
    let mut rejected = 0;
    let mut state = 0x8d14_0a35_bc72_690fu64;
    for n in 0..20000 {
        let mut payload = vec![0; 64 + n % 513];
        for byte in &mut payload {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        if check(&payload, b"{\"s\":\"", b"\"}").is_some() {
            admitted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(admitted > 1000 && rejected > 1000);
    for lead in 0x80..=0xff {
        for offset in 0..=4 {
            let mut payload = b"x".repeat(256);
            payload.push(lead);
            payload.extend(std::iter::repeat_n(0x80, offset));
            check(&payload, b"\"", b"\"");
            payload.extend_from_slice(b"ascii");
            assert!(check(&payload, b"\"", b"\"").is_some());
        }
    }
}

#[test]
fn json_large_source_lengths_preserve_bytes_units_and_tracked_allocation() {
    let minimum = crate::string::JSON_MALLOC_OUTPUT_THRESHOLD as usize;
    for tail in [
        &b"plain"[..],
        "東京🙂".as_bytes(),
        &b"\xed\xa0\x80"[..],
        &b"\xc3"[..],
        &b"\xf0ab"[..],
    ] {
        let mut payload = "Grüße東京🙂".repeat(minimum / 16 + 1).into_bytes();
        payload.extend_from_slice(tail);
        let input = [&b" \n\""[..], &payload, &b"\"\t "[..]].concat();
        let expected_units = compute_utf16_len_wtf8(&payload);
        let source = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        unsafe {
            let value = crate::json::js_json_parse(source);
            let header = value.as_string_ptr();
            assert_eq!((*header).byte_len as usize, payload.len());
            assert_eq!((*header).utf16_len, expected_units);
            let actual =
                std::slice::from_raw_parts(crate::string::string_data(header), payload.len());
            assert_eq!(actual, payload);
            let gc_header = crate::value::addr_class::try_read_gc_header(header as usize).unwrap();
            assert!(crate::gc::gc_malloc_header_is_tracked(gc_header));
        }
    }
}
