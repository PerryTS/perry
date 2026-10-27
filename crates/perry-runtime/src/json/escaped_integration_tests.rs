use crate::string::string_data;

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
                    super::stringify_record_output::try_object(input.bits())
                } else {
                    super::stringify_flat::try_object(input.bits())
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
