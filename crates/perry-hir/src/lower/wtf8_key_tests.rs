// A lone-surrogate string key (`{ "\uD800": 1 }`) keeps its value: it lowers
// as a computed WTF-8 key, never as the empty static key "" (the lossy
// `as_str().unwrap_or("")` the static-key paths used to take).

fn lowered(source: &str) -> String {
    let parsed = perry_parser::parse_typescript(source, "wtf8_key.ts").unwrap();
    let hir = crate::lower_module(&parsed, "wtf8_key", "wtf8_key.ts").unwrap();
    format!("{hir:#?}")
}

#[test]
fn lone_surrogate_object_keys_lower_as_wtf8_computed_keys() {
    for source in [
        "const o = { \"\\uD800\": 1, \"b\": 2 }; console.log(o);",
        "const o = { \"\\uDC00\"() { return 1; } }; console.log(o);",
        "const o = { get \"\\uD800\"() { return 1; } }; console.log(o);",
        "const s = 1; const o = { ...{}, \"\\uD800\": s }; console.log(o);",
    ] {
        let text = lowered(source);
        assert!(text.contains("WtfString("), "WTF-8 key kept: {source}");
        assert!(!text.contains("\"\""), "no empty static key: {source}");
    }
}

#[test]
fn well_formed_string_keys_stay_static() {
    let text = lowered("const o = { \"\u{e9}\": 1, \"a b\": 2 }; console.log(o);");
    assert!(!text.contains("WtfString("));
}

#[test]
fn a_wtf8_key_keeps_its_source_position() {
    // `{ "\uD800": 1, "b": 2 }` defines the WTF-8 key first, as V8 does
    // (JSON.stringify / Object.keys order), not after the static keys.
    let text = lowered("const o = { \"\\uD800\": 1, \"b\": 2 }; console.log(o);");
    // The literal lowers to its source-ordered IIFE: "b" is defined in the
    // body after the WTF-8 key, not pre-built into the IIFE's argument object
    // (which follows the body's `Return`).
    let wtf8 = text.find("WtfString(").expect("WTF-8 key");
    let b = wtf8 + text[wtf8..].find("\"b\"").expect("static key");
    assert!(
        !text[wtf8..b].contains("Return("),
        "\"b\" is defined after the WTF-8 key, inside the ordered body"
    );
}
