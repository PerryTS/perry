//! #5234: native `.wasm` ESM imports.
//!
//! A wasm file is binary, so it cannot enter the normal TypeScript source
//! reader. We parse its export names and synthesize a small TypeScript adapter
//! that embeds the bytes, instantiates the module during module initialization,
//! and re-exports the live instance exports through Perry's normal module
//! pipeline.

/// True when `path` is a `.wasm` file (case-insensitive extension).
pub(crate) fn is_wasm_asset(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wasm"))
        .unwrap_or(false)
}

/// Decode one unsigned LEB128 integer from `bytes` starting at `*pos`.
fn read_uleb128(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
    u32::try_from(result).ok()
}

struct WasmExports {
    names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WasmImport {
    module: String,
    name: String,
}

fn read_name(bytes: &[u8], pos: &mut usize) -> Option<String> {
    let len = read_uleb128(bytes, pos)? as usize;
    let end = pos.checked_add(len)?;
    let name = std::str::from_utf8(bytes.get(*pos..end)?).ok()?.to_string();
    *pos = end;
    Some(name)
}

/// Collect function imports from section 2. The adapter uses these to build
/// the `WebAssembly.instantiate` imports object. A non-function descriptor is
/// left to the host's existing LinkError path until table/memory/global import
/// wrappers are available.
fn parse_wasm_imports(bytes: &[u8]) -> Option<Vec<WasmImport>> {
    const MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6d];
    if bytes.len() < 8 || bytes[0..4] != MAGIC {
        return None;
    }
    let mut pos = 8usize;
    while pos < bytes.len() {
        let section_id = bytes[pos];
        pos += 1;
        let size = read_uleb128(bytes, &mut pos)? as usize;
        let section_end = pos.checked_add(size)?;
        if section_end > bytes.len() {
            return None;
        }
        if section_id != 2 {
            pos = section_end;
            continue;
        }

        let payload = &bytes[pos..section_end];
        let mut import_pos = 0usize;
        let count = read_uleb128(payload, &mut import_pos)?;
        let mut imports = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let module = read_name(payload, &mut import_pos)?;
            let name = read_name(payload, &mut import_pos)?;
            let kind = *payload.get(import_pos)?;
            import_pos += 1;
            if kind != 0 {
                return None;
            }
            let _type_index = read_uleb128(payload, &mut import_pos)?;
            let import = WasmImport { module, name };
            if !imports.contains(&import) {
                imports.push(import);
            }
        }
        return Some(imports);
    }
    Some(Vec::new())
}

/// Walk a wasm binary and collect the names in its export section (id 7).
/// Malformed section data produces an empty list; the host reports the actual
/// compile error later when the synthesized adapter instantiates the bytes.
fn parse_wasm_exports(bytes: &[u8]) -> Option<WasmExports> {
    const MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6d];
    if bytes.len() < 8 || bytes[0..4] != MAGIC {
        return None;
    }

    let mut names = Vec::new();
    let mut pos = 8usize;
    while pos < bytes.len() {
        let section_id = bytes[pos];
        pos += 1;
        let size = match read_uleb128(bytes, &mut pos) {
            Some(size) => size as usize,
            None => break,
        };
        let section_end = match pos.checked_add(size) {
            Some(end) if end <= bytes.len() => end,
            _ => break,
        };
        if section_id == 7 {
            if let Some(found) = parse_export_section(&bytes[pos..section_end]) {
                names = found;
            }
            break;
        }
        pos = section_end;
    }
    Some(WasmExports { names })
}

fn parse_export_section(payload: &[u8]) -> Option<Vec<String>> {
    let mut pos = 0usize;
    let count = read_uleb128(payload, &mut pos)?;
    let mut names = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name_len = read_uleb128(payload, &mut pos)? as usize;
        let name_end = pos.checked_add(name_len)?;
        if name_end > payload.len() {
            return None;
        }
        let name = std::str::from_utf8(&payload[pos..name_end])
            .ok()?
            .to_string();
        pos = name_end;
        let _kind = *payload.get(pos)?;
        pos += 1;
        let _index = read_uleb128(payload, &mut pos)?;
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    Some(names)
}

pub(crate) struct WasmModuleSource {
    pub(crate) source: String,
}

/// Build an executable TypeScript adapter for a `.wasm` import.
///
/// The generated module uses Perry's compile-time `embedWasm` intrinsic, so
/// the produced executable does not need the source wasm file at runtime. The
/// instance exports object is also the default export. Wasm names that cannot
/// be represented as static ESM names remain available through bracket access
/// on that default object.
pub(crate) fn synthesize_wasm_module(bytes: &[u8], display_name: &str) -> WasmModuleSource {
    let names = parse_wasm_exports(bytes)
        .map(|exports| exports.names)
        .unwrap_or_default();
    let imports = parse_wasm_imports(bytes).unwrap_or_default();
    let path_lit = serde_json::to_string(display_name).unwrap_or_else(|_| "\"module.wasm\"".into());

    let mut source = String::new();
    source.push_str("// #5234: synthesized executable adapter for a .wasm import.\n");
    let mut import_groups: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    for (index, import) in imports.iter().enumerate() {
        if !is_valid_js_export_ident(&import.name) {
            continue;
        }
        let local = format!("__perry_wasm_import_{index}");
        let module_lit = serde_json::to_string(&import.module).unwrap_or_else(|_| "\"\"".into());
        source.push_str(&format!(
            "import {{ {} as {local} }} from {module_lit};\n",
            import.name
        ));
        import_groups
            .entry(import.module.clone())
            .or_default()
            .push((import.name.clone(), local));
    }
    if import_groups.is_empty() {
        source.push_str(&format!(
            "const __perry_wasm_result = WebAssembly.instantiate(embedWasm({path_lit}));\n"
        ));
    } else {
        source.push_str("const __perry_wasm_imports = {\n");
        for (module, entries) in &import_groups {
            let module_lit = serde_json::to_string(module).unwrap_or_else(|_| "\"\"".into());
            source.push_str(&format!("  {module_lit}: {{\n"));
            for (name, local) in entries {
                let name_lit = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
                source.push_str(&format!("    {name_lit}: {local},\n"));
            }
            source.push_str("  },\n");
        }
        source.push_str("};\n");
        source.push_str(&format!(
            "const __perry_wasm_result = WebAssembly.instantiate(embedWasm({path_lit}), __perry_wasm_imports);\n"
        ));
    }
    source.push_str("const __perry_wasm_exports = __perry_wasm_result.instance.exports;\n");
    for (index, name) in names.iter().enumerate() {
        if !is_valid_js_export_ident(name) || name == "default" {
            continue;
        }
        let name_lit = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
        let local = format!("__perry_wasm_export_{index}");
        source.push_str(&format!(
            "const {local} = __perry_wasm_exports[{name_lit}];\nexport {{ {local} as {name} }};\n"
        ));
    }
    source.push_str("export default __perry_wasm_exports;\n");

    WasmModuleSource { source }
}

fn is_valid_js_export_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_wasm() -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode("AGFzbQEAAAABBwFgAn9/AX8DAgEABwcBA2FkZAAACgkBBwAgACABags=")
            .unwrap()
    }

    fn imported_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
            0x01, 0x7f, 0x02, 0x0e, 0x01, 0x06, 0x2e, 0x2f, 0x67, 0x6c, 0x75, 0x65, 0x03, 0x69,
            0x6e, 0x63, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x63, 0x61,
            0x6c, 0x6c, 0x00, 0x01, 0x0a, 0x08, 0x01, 0x06, 0x00, 0x20, 0x00, 0x10, 0x00, 0x0b,
        ]
    }

    #[test]
    fn parses_add_export() {
        let bytes = add_wasm();
        assert_eq!(bytes.len(), 41);
        let exports = parse_wasm_exports(&bytes).expect("valid header");
        assert_eq!(exports.names, vec!["add".to_string()]);
    }

    #[test]
    fn synthesizes_instantiated_named_and_default_exports() {
        let source = synthesize_wasm_module(&add_wasm(), "add.wasm").source;
        assert!(source.contains("WebAssembly.instantiate(embedWasm(\"add.wasm\"))"));
        assert!(source.contains("as add"));
        assert!(source.contains("export default __perry_wasm_exports"));
    }

    #[test]
    fn synthesizes_static_function_imports_for_glue_modules() {
        let bytes = imported_wasm();
        assert_eq!(
            parse_wasm_imports(&bytes),
            Some(vec![WasmImport {
                module: "./glue".to_string(),
                name: "inc".to_string(),
            }])
        );
        let source = synthesize_wasm_module(&bytes, "imported.wasm").source;
        assert!(source.contains("import { inc as __perry_wasm_import_0 } from \"./glue\""));
        assert!(source.contains("\"./glue\": {"));
        assert!(source.contains("\"inc\": __perry_wasm_import_0"));
        assert!(source.contains(
            "WebAssembly.instantiate(embedWasm(\"imported.wasm\"), __perry_wasm_imports)"
        ));
        assert!(source.contains("as call"));
    }

    #[test]
    fn malformed_header_still_instantiates_for_a_host_compile_error() {
        assert!(parse_wasm_exports(b"not a wasm file at all").is_none());
        let source = synthesize_wasm_module(b"garbage", "bad.wasm").source;
        assert!(source.contains("WebAssembly.instantiate"));
        assert!(source.contains("export default __perry_wasm_exports"));
        assert!(!source.contains(" as "));
    }

    #[test]
    fn no_export_section_yields_empty_names() {
        let bytes = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let exports = parse_wasm_exports(&bytes).expect("valid header");
        assert!(exports.names.is_empty());
    }

    #[test]
    fn uleb128_multibyte() {
        let bytes = [0xE5u8, 0x8E, 0x26];
        let mut pos = 0;
        assert_eq!(read_uleb128(&bytes, &mut pos), Some(624485));
        assert_eq!(pos, 3);
    }

    #[test]
    fn rejects_non_ident_export_names() {
        assert!(is_valid_js_export_ident("add"));
        assert!(is_valid_js_export_ident("_$foo9"));
        assert!(!is_valid_js_export_ident("9bad"));
        assert!(!is_valid_js_export_ident("has-dash"));
        assert!(!is_valid_js_export_ident(""));
    }
}
