use super::*;

fn units_through_cursor(text: &str, order: &[usize]) -> Vec<i32> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_string_ptr(js_string_from_str(text));
    let mut cursor = Utf16Cursor {
        byte_len: 0,
        byte: 0,
        utf16: 0,
    };
    s.with_const_ptr(|s_ptr: *const StringHeader| {
        order
            .iter()
            .map(|&i| js_string_utf16_unit_at_cursor(s_ptr, i as i32, &mut cursor))
            .collect()
    })
}

#[test]
fn every_order_reads_the_utf16_unit() {
    let text = "a é ü – 😀 z中";
    let expected: Vec<i32> = text.encode_utf16().map(i32::from).collect();
    let forward: Vec<usize> = (0..expected.len()).collect();
    assert_eq!(units_through_cursor(text, &forward), expected);
    let backward: Vec<usize> = (0..expected.len()).rev().collect();
    let mut reversed = expected.clone();
    reversed.reverse();
    assert_eq!(units_through_cursor(text, &backward), reversed);
    // Both halves of the astral character, a repeat and a backward jump.
    let mixed = [9, 8, 9, 10, 3, 3, 12, 0];
    let want: Vec<i32> = mixed.iter().map(|&i| expected[i]).collect();
    assert_eq!(units_through_cursor(text, &mixed), want);
}

#[test]
fn a_sequential_read_resumes_at_the_previous_code_point() {
    let text = "é".repeat(64);
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_string_ptr(js_string_from_str(&text));
    let mut cursor = Utf16Cursor {
        byte_len: 0,
        byte: 0,
        utf16: 0,
    };
    s.with_const_ptr(|s_ptr: *const StringHeader| {
        for i in 0..64 {
            assert_eq!(js_string_utf16_unit_at_cursor(s_ptr, i, &mut cursor), 0xe9);
            assert_eq!((cursor.byte, cursor.utf16), (2 * i as u32, i as u32));
        }
    });
}

#[test]
fn a_changed_payload_length_or_a_missing_unit_restarts_or_declines() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_string_ptr(js_string_from_str("ééé"));
    let mut cursor = Utf16Cursor {
        byte_len: 99,
        byte: 40,
        utf16: 1,
    };
    s.with_const_ptr(|s_ptr: *const StringHeader| {
        assert_eq!(js_string_utf16_unit_at_cursor(s_ptr, 2, &mut cursor), 0xe9);
        assert_eq!((cursor.byte_len, cursor.byte, cursor.utf16), (6, 4, 2));
        assert_eq!(js_string_utf16_unit_at_cursor(s_ptr, 3, &mut cursor), -1);
        assert_eq!(js_string_utf16_unit_at_cursor(s_ptr, -1, &mut cursor), -1);
    });
}
