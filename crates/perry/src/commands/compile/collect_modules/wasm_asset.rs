//! #5235: defer `.wasm` ESM imports.
//!
//! An `import ... from "./x.wasm"` cannot run in an ahead-of-time compiled
//! binary today — real `.wasm` ESM instantiation is tracked as the companion
//! issue #5234. Rather than hard-failing the whole build (the file isn't valid
//! UTF-8, so the normal TS read aborts with `stream did not contain valid
//! UTF-8`), we *defer* the import: we parse the WebAssembly **export section**
//! for the export names and synthesize a tiny TypeScript module whose exports
//! are **throw-on-call stubs**. The build proceeds past peripheral `.wasm`
//! dependencies; the wasm feature throws a descriptive `Error` only if actually
//! reached.
//!
//! This mirrors the #5206 / #5230 deferred-AOT-site policy: the site is recorded
//! in the shared end-of-compile notice (`record_deferred_aot_site`), and strict
//! mode (`perry.strict` / `--strict-dynamic-import`) turns it into a hard
//! compile error instead.
//!
//! The export-section walk is a trivial, defensive binary parse — on *any*
//! malformed input we fall back to synthesizing just a throwing default export
//! (and still record the deferred site) rather than crashing.

/// True when `path` is a `.wasm` file (case-insensitive extension).
pub(crate) fn is_wasm_asset(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wasm"))
        .unwrap_or(false)
}

/// Decode one unsigned LEB128 integer from `bytes` starting at `*pos`.
/// Advances `*pos` past the consumed bytes. Returns `None` on truncation or an
/// over-long encoding (more than 5 bytes for the u32 range we care about — the
/// wasm spec caps section/name/index encodings at u32).
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
        // u32 LEB128 is at most 5 bytes; guard against unbounded/over-long input.
        if shift >= 35 {
            return None;
        }
    }
    u32::try_from(result).ok()
}

/// Parsed export-section result.
struct WasmExports {
    /// Export names found in section id 7. Empty when the section is absent.
    names: Vec<String>,
}

/// Walk a `.wasm` binary and collect the names in its export section (id 7).
///
/// Returns `None` when the header is absent/malformed (caller then synthesizes a
/// default-only stub). Returns `Some(WasmExports { names })` — possibly with an
/// empty `names` vec — when the header is valid; a parse error *inside* a
/// section stops the walk but keeps whatever names were collected so far.
fn parse_wasm_exports(bytes: &[u8]) -> Option<WasmExports> {
    // Header: 4-byte magic `\0asm` + 4-byte version `01 00 00 00`.
    const MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6d];
    if bytes.len() < 8 || bytes[0..4] != MAGIC {
        return None;
    }
    // We don't enforce the exact version bytes — any 8-byte-or-longer module
    // with the right magic is walked; an unknown version simply won't contain a
    // recognizable export section and yields an empty name list.

    let mut names: Vec<String> = Vec::new();
    let mut pos = 8usize;
    while pos < bytes.len() {
        let section_id = bytes[pos];
        pos += 1;
        let size = match read_uleb128(bytes, &mut pos) {
            Some(s) => s as usize,
            None => break, // truncated section header — stop, keep what we have
        };
        let section_start = pos;
        let section_end = match section_start.checked_add(size) {
            Some(end) if end <= bytes.len() => end,
            _ => break, // section claims more bytes than the file has
        };
        if section_id == 7 {
            // Export section: uleb count, then `count` entries of
            // (uleb name_len, name_len bytes, 1 byte kind, uleb index).
            if let Some(found) = parse_export_section(&bytes[section_start..section_end]) {
                names = found;
            }
            // Export section appears at most once; we can stop after it.
            break;
        }
        pos = section_end;
    }
    Some(WasmExports { names })
}

/// Parse the payload of an export section (everything after the section id +
/// size header). Returns the collected export names, or `None` on a parse error
/// (caller keeps the prior — empty — name list and falls back to default-only).
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
        let name = std::str::from_utf8(&payload[pos..name_end]).ok()?.to_string();
        pos = name_end;
        // 1-byte export kind (0 = func, 1 = table, 2 = mem, 3 = global), then
        // the uleb index. We include *every* kind as a throwing function stub —
        // simplest, and member access only throws when actually invoked.
        let _kind = *payload.get(pos)?;
        pos += 1;
        let _index = read_uleb128(payload, &mut pos)?;
        // Skip empty / duplicate names defensively.
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    Some(names)
}

/// Result of synthesizing a deferred `.wasm` stub module.
pub(crate) struct WasmStubModule {
    /// Synthesized TypeScript source — flows through the normal parse/lower
    /// pipeline exactly like the #5223 text-asset / JSON synthetic modules.
    pub(crate) source: String,
}

/// Build a throwing-stub TypeScript module for a `.wasm` import (#5235).
///
/// `bytes` is the raw `.wasm` file content; `display_name` is the file name (or
/// path) used in the thrown error message. Every export name parsed from the
/// module's export section becomes a named export whose value is a function that
/// throws a descriptive `Error` when called; a throwing **default** export is
/// always provided too (covers `import w from "./x.wasm"`). On a malformed
/// binary we synthesize the default-only stub.
///
/// Returns the synthesized source. Does not record the deferred site or consult
/// strict mode — the caller does both, so it can decide between erroring and
/// deferring.
pub(crate) fn synthesize_wasm_stub_module(bytes: &[u8], display_name: &str) -> WasmStubModule {
    let names = parse_wasm_exports(bytes)
        .map(|e| e.names)
        .unwrap_or_default();
    // The descriptive runtime message. JS-string-escaped via serde so the file
    // name (which may contain quotes/odd chars) is safe to embed.
    let msg = format!(
        "wasm module {} cannot run in an ahead-of-time compiled binary \
         — full .wasm ESM instantiation is tracked in #5234",
        display_name
    );
    let msg_lit = serde_json::to_string(&msg).unwrap_or_else(|_| "\"wasm module unavailable\"".into());

    let mut src = String::new();
    src.push_str("// #5235: synthesized deferred stub for a .wasm import.\n");
    src.push_str("// Each export throws only when actually called; real .wasm ESM\n");
    src.push_str("// instantiation is tracked in #5234.\n");
    // A single shared thrower keeps the synthesized module compact regardless of
    // export count.
    src.push_str(&format!(
        "function __perry_wasm_unavailable(): never {{ throw new Error({}); }}\n",
        msg_lit
    ));
    for name in &names {
        if !is_valid_js_export_ident(name) {
            // Names that aren't valid bare JS identifiers can't be exported as
            // `export function <name>`. Skip them — they're reachable through
            // the namespace object's string keys only via real instantiation
            // (#5234); for the deferred stub, omitting them is fine. The default
            // export still throws.
            continue;
        }
        src.push_str(&format!(
            "export function {}(...args: any[]): any {{ return __perry_wasm_unavailable(); }}\n",
            name
        ));
    }
    // Throwing default export: a function so `import w from "./x.wasm"; w()`
    // throws on call, and bare `import w from "./x.wasm"` (no call) is fine.
    src.push_str("export default function (...args: any[]): any { return __perry_wasm_unavailable(); }\n");

    WasmStubModule { source: src }
}

/// A name is exportable as `export function <name>` only if it's a valid bare
/// ECMAScript identifier: first char is a letter / `_` / `$`, the rest are
/// alphanumeric / `_` / `$`. (The wasm export-name grammar is far broader.)
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

    /// The fixture from #5235: a 41-byte wasm module exporting `add`.
    fn add_wasm() -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode("AGFzbQEAAAABBwFgAn9/AX8DAgEABwcBA2FkZAAACgkBBwAgACABags=")
            .unwrap()
    }

    #[test]
    fn parses_add_export() {
        let bytes = add_wasm();
        assert_eq!(bytes.len(), 41, "fixture should be 41 bytes");
        let exports = parse_wasm_exports(&bytes).expect("valid header");
        assert_eq!(exports.names, vec!["add".to_string()]);
    }

    #[test]
    fn synthesizes_named_and_default_stub() {
        let src = synthesize_wasm_stub_module(&add_wasm(), "add.wasm").source;
        assert!(src.contains("export function add("), "named stub present");
        assert!(src.contains("export default function"), "default stub present");
        assert!(src.contains("#5234"), "references real-integration issue");
        assert!(src.contains("add.wasm"), "names the file in the message");
    }

    #[test]
    fn malformed_header_falls_back_to_default_only() {
        // Wrong magic → no header.
        assert!(parse_wasm_exports(b"not a wasm file at all").is_none());
        let src = synthesize_wasm_stub_module(b"garbage", "bad.wasm").source;
        // Default export still throws; no named exports synthesized.
        assert!(src.contains("export default function"));
        assert!(!src.contains("export function "));
    }

    #[test]
    fn no_export_section_yields_empty_names() {
        // Valid header + version, no sections.
        let bytes = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let exports = parse_wasm_exports(&bytes).expect("valid header");
        assert!(exports.names.is_empty());
    }

    #[test]
    fn uleb128_multibyte() {
        // 624485 = 0xE5 0x8E 0x26 in uleb128 (the canonical spec example).
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
