use super::*;

#[test]
fn json_escaped_expansion_count_matches_scalar_oracle_at_every_lane_and_tail() {
    let oracle = |bytes: &[u8]| -> u64 {
        bytes
            .iter()
            .map(|b| match b {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 1,
                0..=31 => 5,
                _ => 0,
            })
            .sum()
    };
    for len in 0..=80 {
        for value in 0..=255u8 {
            let bytes = vec![value; len];
            assert_eq!(count_expansion(&bytes), oracle(&bytes));
        }
    }
    let mut state = 0x27f1_065d_99a8_c31bu64;
    for offset in 0..32 {
        let mut bytes = vec![0; 4096 + offset];
        for b in &mut bytes {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *b = state as u8;
        }
        for tail in 0..32 {
            let slice = &bytes[offset..bytes.len() - tail];
            assert_eq!(count_expansion(slice), oracle(slice));
        }
    }
}

#[test]
fn json_escaped_output_matches_complete_oracle_with_bounded_writes() {
    let ascii: String = (0..=127).map(char::from).collect();
    for text in [
        ascii,
        "\"\\\n\r\t\u{8}\u{c}東京🙂\u{2028}\u{2029}".repeat(4096),
    ] {
        let expected = serde_json::to_string(&text).unwrap();
        let plan = Plan::new(text.as_bytes(), text.encode_utf16().count() as u32).unwrap();
        let mut output = vec![0xa5; plan.bytes as usize + 32];
        let written = unsafe { plan.write(text.as_ptr(), output.as_mut_ptr().add(16)) };
        assert_eq!(written, expected.len());
        assert_eq!(&output[16..16 + written], expected.as_bytes());
        assert!(output[..16]
            .iter()
            .chain(&output[16 + written..])
            .all(|&b| b == 0xa5));
        assert_eq!(plan.units as usize, expected.encode_utf16().count());
        unsafe {
            let source = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let result = quote(source).unwrap();
            assert_eq!(
                std::slice::from_raw_parts(string_data(result), (*result).byte_len as usize),
                expected.as_bytes()
            );
            assert_eq!((*result).utf16_len, plan.units);
            assert_eq!((*result).capacity, plan.bytes);
            assert_eq!((*result).flags, 0);
        }
    }
}

#[test]
fn json_escaped_output_declines_invalid_utf8_and_length_overflow() {
    for bytes in [
        b"\n\xed\xa0\x80".as_slice(),
        b"\n\xed\xa0\xbd\xed\xb1\x8d",
        b"\"\x80",
        b"\n\xc2",
        b"\n\xe2\x82",
        b"\n\xf0\x9f\x99",
        b"\n\xff",
    ] {
        assert!(Plan::new(bytes, bytes.len() as u32).is_none());
    }
    assert!(Plan::with_expansion(u32::MAX - 1, 0, 0).is_none());
    assert!(Plan::with_expansion(0, u32::MAX - 1, 0).is_none());
    assert!(Plan::with_expansion(0, 0, u64::MAX).is_none());
    assert!(Plan::with_expansion(0, 0, u32::MAX as u64).is_none());
    assert_eq!(
        Plan::with_expansion(u32::MAX - 7, 0, 5).unwrap().bytes,
        u32::MAX
    );
}

#[test]
fn json_escaped_output_handles_every_ascii_byte_in_keys_values_and_arrays() {
    unsafe {
        for byte in 0..=127u8 {
            let text = format!("prefix{}東京🙂suffix", char::from(byte));
            let quoted = serde_json::to_string(&text).unwrap();
            for array in [false, true] {
                let field = if array {
                    format!("[{quoted},1,true,null]")
                } else {
                    quoted.clone()
                };
                let expected = format!("{{{quoted}:{field},\"id\":1}}");
                let source =
                    crate::string::js_string_from_bytes(expected.as_ptr(), expected.len() as u32);
                let input = crate::json::test_json_parse_direct(source);
                let result = if array {
                    super::super::stringify_record_output::try_object(input.bits())
                } else {
                    super::super::stringify_flat::try_object(input.bits())
                }
                .expect("direct output must accept escaped keys and fields");
                let header = result.as_string_ptr();
                assert_eq!(
                    std::slice::from_raw_parts(string_data(header), (*header).byte_len as usize),
                    expected.as_bytes()
                );
                assert_eq!(
                    (*header).utf16_len as usize,
                    expected.encode_utf16().count()
                );
            }
        }
    }
}

#[test]
fn json_native_buffer_uses_bounded_plans_and_retains_suffix_capacity() {
    for prefix in [0, 15, 255, 1023] {
        for text in [
            "line\n\"quote\"\\tab\t".repeat(4096),
            "東京🙂한\n\u{1}".repeat(1024),
            "plain".repeat(1000),
        ] {
            let quoted = serde_json::to_string(&text).unwrap();
            let mut output = String::with_capacity(257);
            output.extend(std::iter::repeat_n('p', prefix));
            assert!(unsafe { append_to_native_buffer(&mut output, text.as_bytes()) });
            let capacity = output.capacity();
            output.push_str("\n}");
            assert_eq!(output.capacity(), capacity, "closing punctuation must fit");
            assert_eq!(output, "p".repeat(prefix) + quoted.as_str() + "\n}");
        }
    }
    for input in [b"\xed\xa0\x80".as_slice(), b"\xff", b"\n\xc2"] {
        let mut output = String::from("kept prefix");
        let capacity = output.capacity();
        assert!(!unsafe { append_to_native_buffer(&mut output, input) });
        assert_eq!(output, "kept prefix");
        assert_eq!(output.capacity(), capacity);
    }
}

#[cfg(all(target_arch = "aarch64", unix))]
#[test]
fn json_native_vector_escape_respects_exact_source_and_output_guard_pages() {
    struct GuardedPage {
        base: *mut u8,
        page: usize,
    }
    impl GuardedPage {
        unsafe fn new() -> Self {
            let size = libc::sysconf(libc::_SC_PAGESIZE);
            assert!(size > 0);
            let page = size as usize;
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
            let result = Self { base, page };
            assert_eq!(
                libc::mprotect(base.add(page).cast(), page, libc::PROT_NONE),
                0
            );
            result
        }
    }
    impl Drop for GuardedPage {
        fn drop(&mut self) {
            assert_eq!(unsafe { libc::munmap(self.base.cast(), self.page * 2) }, 0);
        }
    }
    unsafe {
        let source = GuardedPage::new();
        let output = GuardedPage::new();
        let check = |text: &str| {
            let expected = serde_json::to_string(text).unwrap();
            let plan = Plan::new(text.as_bytes(), text.encode_utf16().count() as u32).unwrap();
            assert_eq!(plan.bytes as usize, expected.len());
            assert!(text.len() < source.page && expected.len() < output.page);
            assert_eq!(
                libc::mprotect(
                    source.base.cast(),
                    source.page,
                    libc::PROT_READ | libc::PROT_WRITE
                ),
                0
            );
            let input = source.base.add(source.page - text.len());
            std::slice::from_raw_parts_mut(input, text.len()).copy_from_slice(text.as_bytes());
            assert_eq!(
                libc::mprotect(source.base.cast(), source.page, libc::PROT_READ),
                0
            );
            let destination = output.base.add(output.page - expected.len());
            std::slice::from_raw_parts_mut(output.base, output.page).fill(0xa5);
            let written =
                native_escape::write(std::slice::from_raw_parts(input, text.len()), destination);
            assert_eq!(written, expected.len());
            assert_eq!(
                std::slice::from_raw_parts(destination, written),
                expected.as_bytes()
            );
            assert!(
                std::slice::from_raw_parts(output.base, output.page - written)
                    .iter()
                    .all(|&byte| byte == 0xa5)
            );
        };
        for length in 0..=257 {
            check(&"a".repeat(length));
        }
        // Exercise every placement of common escapes in eight adjacent bytes,
        // including pairs that straddle a vector boundary.
        for placement in 0..256 {
            for offset in 0..16 {
                let mut text = "a".repeat(offset);
                let escapes = ['"', '\\', '\n', '\r', '\t', '\u{8}', '\u{c}'];
                for lane in 0..8 {
                    text.push(if placement & (1 << lane) == 0 {
                        'z'
                    } else {
                        escapes[(placement + lane) % escapes.len()]
                    });
                }
                text.push_str(&"b".repeat(16 - offset));
                check(&text);
            }
        }
        for byte in 0..=127u8 {
            for offset in 0..32 {
                check(&format!(
                    "{}{}東京🙂한\\\n\u{1}{}",
                    "a".repeat(offset),
                    char::from(byte),
                    "z".repeat(31 - offset)
                ));
            }
            check(&char::from(byte).to_string().repeat(257));
        }
        for repetitions in [1, 7, 15, 16, 17, 31, 32, 33, 95] {
            check(&"\"\\\n\r\t\u{8}\u{c}\u{1}東京🙂한".repeat(repetitions));
        }
    }
}
