use super::*;

fn string_metadata(entries: &[TapeEntry]) -> Vec<(u8, u32)> {
    entries
        .iter()
        .filter(|entry| matches!(entry.kind, KIND_KEY | KIND_STRING))
        .map(|entry| (entry.kind, entry.link))
        .collect()
}

#[test]
fn tape_string_metadata_covers_keys_values_and_every_escape() {
    for (token, plain) in [
        (r#""""#, true),
        (r#""hello""#, true),
        (r#""é😀 / space""#, true),
        (r#""a\"b""#, false),
        (r#""a\\b""#, false),
        (r#""a\/b""#, false),
        (r#""\b""#, false),
        (r#""\f""#, false),
        (r#""\n""#, false),
        (r#""\r""#, false),
        (r#""\t""#, false),
        (r#""\u0061""#, false),
        (r#""\ud83d\ude00""#, false),
        (r#""\uD800""#, false),
    ] {
        let source = format!(r#"[{{{token}:{token},"plain":{token}}},{token}]"#);
        let tape = build_tape(source.as_bytes()).expect("valid fixture");
        let metadata = if plain { STRING_NO_ESCAPES } else { 0 };
        assert_eq!(
            string_metadata(&tape.entries),
            [
                (KIND_KEY, metadata),
                (KIND_STRING, metadata),
                (KIND_KEY, STRING_NO_ESCAPES),
                (KIND_STRING, metadata),
                (KIND_STRING, metadata),
            ],
            "{source}"
        );
        assert_eq!(count_array_length(&tape.entries, 0), 2);
    }
}

#[test]
fn tape_string_metadata_clears_after_simd_prefixes_and_vector_growth() {
    for prefix in [0, 1, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64] {
        let source = format!(r#"["{}\nend","plain"]"#, "x".repeat(prefix));
        let tape = build_tape(source.as_bytes()).unwrap();
        assert_eq!(
            string_metadata(&tape.entries),
            [(KIND_STRING, 0), (KIND_STRING, STRING_NO_ESCAPES)]
        );
    }
    // Dense empty strings exceed the builder's len/8 reservation and grow the
    // Vec between tokens. A prior token's metadata must not follow a stale slot.
    let source = format!("[{}]", vec![r#""""#; 2500].join(","));
    let tape = build_tape(source.as_bytes()).unwrap();
    assert_eq!(count_array_length(&tape.entries, 0), 2500);
    assert_eq!(
        string_metadata(&tape.entries),
        vec![(KIND_STRING, STRING_NO_ESCAPES); 2500]
    );
}

#[test]
fn tape_string_metadata_survives_both_depth_modes_and_native_transfer() {
    let source = br#"[{"plain":["x",{"next":"\u0078"}]},"y"]"#;
    let expected = build_tape(source).unwrap().entries;
    let mut scratch = TapeScratch::new();
    let mut depth = 0;
    assert!(build_tape_into::<true>(
        source,
        &mut scratch.entries,
        &mut scratch.stack,
        &mut depth
    ));
    assert_eq!(depth, 4);
    assert_eq!(scratch.entries, expected);
    let owned = std::mem::take(&mut scratch.entries);
    assert!(build_tape_into::<false>(
        br#"["\n"]"#,
        &mut scratch.entries,
        &mut scratch.stack,
        &mut depth
    ));
    assert_eq!(string_metadata(&scratch.entries), [(KIND_STRING, 0)]);
    scratch.trim_for_reuse();
    assert_eq!(owned, expected);
    let borrowed = with_built_tape(source, |entries| entries.to_vec()).unwrap();
    assert_eq!(borrowed, expected);
}

#[test]
fn failed_string_scans_do_not_publish_or_contaminate_scratch() {
    for source in [
        br#"["abc"#.as_slice(),
        br#"["abc\"#.as_slice(),
        br#"[{"a\q":1}]"#.as_slice(),
        br#"[{"first":0,"next":"\u00x0"}]"#.as_slice(),
        b"[\"raw\ncontrol\"]".as_slice(),
    ] {
        let called = Cell::new(false);
        assert!(with_built_tape(source, |_| called.set(true)).is_none());
        assert!(!called.get(), "failed tape reached a consumer");
        let valid = with_built_tape(br#"[{"ok":"yes","next":"\n"}]"#, string_metadata)
            .expect("scratch reusable after a malformed string");
        assert_eq!(
            valid,
            [
                (KIND_KEY, STRING_NO_ESCAPES),
                (KIND_STRING, STRING_NO_ESCAPES),
                (KIND_KEY, STRING_NO_ESCAPES),
                (KIND_STRING, 0),
            ]
        );
        let mut entries = Vec::new();
        let mut stack = Vec::new();
        assert!(!build_tape_into::<true>(
            source,
            &mut entries,
            &mut stack,
            &mut 0
        ));
    }
}

#[test]
fn plain_string_metadata_does_not_claim_unicode_validity() {
    let source = b"[\"\xed\xa0\x80\"]";
    let tape = build_tape(source).expect("syntax validation does not decode UTF-8");
    assert_eq!(
        string_metadata(&tape.entries),
        [(KIND_STRING, STRING_NO_ESCAPES)]
    );
    assert!(std::str::from_utf8(source).is_err());
}
