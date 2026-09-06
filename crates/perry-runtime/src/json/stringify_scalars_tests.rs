use super::write_escaped_string;

#[test]
fn vector_escaping_matches_json_encoder_at_every_boundary() {
    let characters: Vec<char> = (0..=127)
        .filter_map(char::from_u32)
        .chain(['é', '東', '🙂', '\u{D000}', '\u{D7FF}', '\u{E000}'])
        .collect();
    for prefix_len in [0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 63, 64, 255, 4096] {
        let prefix = "a".repeat(prefix_len);
        for &ch in &characters {
            let text = format!("{prefix}{ch}last");
            let mut output = String::new();
            unsafe { write_escaped_string(&mut output, &text) };
            assert_eq!(
                output,
                serde_json::to_string(&text).unwrap(),
                "prefix={prefix_len} ch={ch:?}"
            );
        }
    }
}

#[test]
fn control_escapes_append_to_existing_output_without_temporary_formatting() {
    let text: String = (0..32).filter_map(char::from_u32).collect();
    let mut output = String::from("prefix:");
    unsafe { write_escaped_string(&mut output, &text) };
    assert_eq!(
        output,
        format!("prefix:{}", serde_json::to_string(&text).unwrap())
    );
}
