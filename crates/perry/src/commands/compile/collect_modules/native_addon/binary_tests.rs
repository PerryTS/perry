use super::validate_node_api_binary;

/// One imported symbol in a minimal two-level Mach-O image. Keep the symbol
/// range separate from its library ordinal: dynamic lookup uses 0xfe, and
/// executable lookup uses 0xff, neither of which is a dylib-table index.
fn macho_import(name: &str, ordinal: u8, is_64: bool, symbol_type: u8) -> Vec<u8> {
    let header_size = if is_64 { 32 } else { 28 };
    let symbol_size = if is_64 { 16 } else { 12 };
    let symbol_offset = header_size + 104;
    let string_offset = symbol_offset + symbol_size;
    let mut bytes = Vec::new();
    let header = [
        if is_64 { 0xfeedfacfu32 } else { 0xfeedface },
        if is_64 { 0x01000007 } else { 7 },
        3, // CPU subtype
        6, // MH_DYLIB
        2, // LC_SYMTAB and LC_DYSYMTAB
        104,
        0x80, // MH_TWOLEVEL
    ];
    for word in header {
        bytes.extend(word.to_le_bytes());
    }
    if is_64 {
        bytes.extend(0u32.to_le_bytes());
    }
    for word in [
        2u32,
        24,
        symbol_offset,
        1,
        string_offset,
        name.len() as u32 + 2,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    let mut dysymtab = [0u32; 20];
    dysymtab[0] = 11;
    dysymtab[1] = 80;
    dysymtab[7] = 1; // nundefsym; iundefsym is zero
    for word in dysymtab {
        bytes.extend(word.to_le_bytes());
    }
    assert_eq!(bytes.len(), symbol_offset as usize);
    bytes.extend(1u32.to_le_bytes()); // n_strx skips the initial NUL
    bytes.extend([symbol_type, 0, 0, ordinal]);
    bytes.extend(vec![0; if is_64 { 8 } else { 4 }]);
    bytes.push(0);
    bytes.extend(name.as_bytes());
    bytes.push(0);
    bytes
}

fn validate(bytes: &[u8]) -> anyhow::Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("addon.node");
    std::fs::write(&path, bytes).unwrap();
    validate_node_api_binary(&path)
}

#[test]
fn macho_dynamic_and_executable_lookup_accept_node_api() {
    for is_64 in [false, true] {
        for ordinal in [0xfe, 0xff] {
            validate(&macho_import("_napi_create_int32", ordinal, is_64, 1))
                .unwrap_or_else(|error| panic!("64-bit={is_64}, ordinal={ordinal}: {error}"));
        }
    }
}

#[test]
fn macho_special_ordinals_still_reject_unsupported_imports() {
    for is_64 in [false, true] {
        for name in [
            "_uv_run",
            "__ZN2v812some_function",
            "__ZN4node8Function",
            "Nan::Call",
        ] {
            for ordinal in [0xfe, 0xff] {
                let error = validate(&macho_import(name, ordinal, is_64, 1))
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains("imports unsupported symbol"),
                    "{name}: {error}"
                );
                assert!(error.contains(name), "{error}");
            }
        }
    }
}

#[test]
fn macho_prebound_undefined_import_is_not_skipped() {
    let error = validate(&macho_import("_uv_run", 0xfe, true, 0x0d))
        .unwrap_err()
        .to_string();
    assert!(error.contains("imports unsupported symbol"), "{error}");
}

#[test]
fn macho_invalid_import_range_is_rejected() {
    let mut bytes = macho_import("_napi_create_int32", 0, true, 1);
    bytes[84..88].copy_from_slice(&2u32.to_le_bytes());
    let error = validate(&bytes).unwrap_err().to_string();
    assert!(error.contains("symbol index"), "{error}");
}
