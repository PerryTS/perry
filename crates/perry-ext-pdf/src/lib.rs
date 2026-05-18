//! `@perryts/pdf` native bindings (issue #516).
//!
//! Minimal PDF *creation* API. The viewer half of #516 (PdfView widget
//! across iOS/visionOS/macOS + stubs on the other platform crates) is
//! already shipped — this crate is the producer side.
//!
//! TypeScript surface:
//!
//! ```ignore
//! export declare function createPdf(opts: {
//!     path: string;
//!     pageWidth?: number;
//!     pageHeight?: number;
//! }): number;
//! export declare function pdfAddText(
//!     pdf: number, text: string, x: number, y: number, fontSize?: number,
//! ): void;
//! export declare function pdfAddLine(
//!     pdf: number, x1: number, y1: number, x2: number, y2: number,
//! ): void;
//! export declare function pdfNewPage(pdf: number): void;
//! export declare function pdfSave(pdf: number): void;
//! ```
//!
//! Page units are PDF points (1/72 inch). The default page is US
//! Letter (612 × 792 pt). The origin is the bottom-left corner — that
//! is the PDF native coordinate system, not a Perry convention.
//!
//! # Handle model
//!
//! `createPdf` returns a 1-based `i64` handle into a process-global
//! `Mutex<HashMap<i64, OpenDoc>>`. Each `OpenDoc` carries the in-
//! progress `printpdf::PdfDocument`, the current page's accumulated
//! `Op` list, the page dimensions, and the destination path. The
//! mutation methods (`pdfAddText`, `pdfAddLine`, `pdfNewPage`) look up
//! the handle, mutate the in-memory state, and return. `pdfSave`
//! flushes the current page, serializes the document, writes the
//! file, and removes the entry from the table — subsequent calls on
//! the freed handle become no-ops with a `warn-once` log line.

use std::collections::HashMap;
use std::fs;
use std::sync::{Mutex, OnceLock};

use perry_ffi::{read_string, JsString, StringHeader};
use printpdf::{
    BuiltinFont, Color, Line, LinePoint, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions,
    PdfWarnMsg, Point, Pt, Rgb, TextItem,
};

// ============================================================================
// State
// ============================================================================

/// Per-handle PDF-in-progress.
struct OpenDoc {
    /// Path to write on `pdfSave`. Captured at `createPdf` time so the
    /// save call doesn't have to re-thread it.
    path: String,
    /// Page size in PDF points. Applied to every page, including ones
    /// added later via `pdfNewPage`.
    page_width_pt: f32,
    page_height_pt: f32,
    /// Pages already finalized. The current (in-progress) page is held
    /// separately in `current_ops` to avoid clone-on-every-mutation.
    finished_pages: Vec<PdfPage>,
    /// Ops accumulated on the page currently being drawn.
    current_ops: Vec<Op>,
}

impl OpenDoc {
    /// Finalize the current page and start a fresh op buffer. Idempotent
    /// when the current page is empty — `pdfNewPage` called twice in a
    /// row does NOT emit two blank pages.
    fn finalize_current_page(&mut self) {
        if self.current_ops.is_empty() {
            return;
        }
        let ops = std::mem::take(&mut self.current_ops);
        let page = PdfPage::new(
            Pt(self.page_width_pt).into(),
            Pt(self.page_height_pt).into(),
            ops,
        );
        self.finished_pages.push(page);
    }
}

/// Process-global state. `OnceLock` here defers Mutex creation to first
/// use; the FFI never touches it from multiple threads concurrently in
/// the v1 surface (the JS runtime is single-threaded), but using a
/// `Mutex` keeps the door open for `perry/thread`-style parallelism
/// later without changing the FFI contract.
fn state() -> &'static Mutex<HashMap<i64, OpenDoc>> {
    static STATE: OnceLock<Mutex<HashMap<i64, OpenDoc>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Monotonic handle counter. Starts at 1 so a zero return value is
/// always unambiguously an error.
fn next_handle() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static COUNTER: AtomicI64 = AtomicI64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Default US Letter at 1/72 inch.
const DEFAULT_PAGE_WIDTH_PT: f32 = 612.0;
const DEFAULT_PAGE_HEIGHT_PT: f32 = 792.0;
const DEFAULT_FONT_SIZE_PT: f32 = 12.0;

/// Warn once per stale handle id so a buggy caller doesn't spam logs.
fn warn_stale_handle_once(handle: i64) {
    use std::collections::HashSet;
    static WARNED: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut set) = warned.lock() {
        if set.insert(handle) {
            eprintln!(
                "[perry-ext-pdf] warning: operation on stale PDF handle {} \
                 (already saved or never created)",
                handle
            );
        }
    }
}

// ============================================================================
// FFI: createPdf
// ============================================================================

/// `createPdf({ path, pageWidth?, pageHeight? })`.
///
/// Codegen lowers this as `js_pdf_create_pdf(opts_nan_boxed_f64)` where
/// `opts_nan_boxed_f64` is the NaN-boxed JSValue pointing at the
/// `{ path, pageWidth?, pageHeight? }` object. We round-trip through
/// `perry_ffi::json_stringify` (same trick fastify uses for its config
/// object) so this crate doesn't depend on `perry-runtime` object
/// internals.
///
/// Returns the new handle as an `i64` (NaN-boxed at the call site with
/// `POINTER_TAG` by the `NR_PTR` codegen path). Returns 0 on bad input
/// — codegen NaN-boxes 0 as a small pointer, which the JS side sees as
/// a number that subsequent ops will treat as a stale handle and skip.
#[no_mangle]
pub unsafe extern "C" fn js_pdf_create_pdf(opts_f64: f64) -> i64 {
    let opts_value = perry_ffi::JsValue::from_bits(opts_f64.to_bits());
    let json = match perry_ffi::json_stringify(opts_value) {
        Some(s) => s,
        None => {
            eprintln!("[perry-ext-pdf] createPdf: options object is not stringifiable");
            return 0;
        }
    };
    let parsed: serde_json_lite::Value = match serde_json_lite::parse(&json) {
        Some(v) => v,
        None => {
            eprintln!("[perry-ext-pdf] createPdf: failed to parse options JSON");
            return 0;
        }
    };

    let path = match parsed.get_str("path") {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            eprintln!("[perry-ext-pdf] createPdf: `path` option is required");
            return 0;
        }
    };

    let page_width_pt = parsed
        .get_f32("pageWidth")
        .filter(|n| *n > 0.0)
        .unwrap_or(DEFAULT_PAGE_WIDTH_PT);
    let page_height_pt = parsed
        .get_f32("pageHeight")
        .filter(|n| *n > 0.0)
        .unwrap_or(DEFAULT_PAGE_HEIGHT_PT);

    let handle = next_handle();
    let mut guard = match state().lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    guard.insert(
        handle,
        OpenDoc {
            path,
            page_width_pt,
            page_height_pt,
            finished_pages: Vec::new(),
            current_ops: Vec::new(),
        },
    );
    handle
}

// ============================================================================
// FFI: pdfAddText
// ============================================================================

/// `pdfAddText(pdf, text, x, y, fontSize?)`.
///
/// Emits the printpdf op sequence for placing one literal string at
/// `(x, y)` in Helvetica (one of the 14 PDF built-in fonts — no font
/// file required). `fontSize` is in points; defaults to 12 when 0 or
/// negative.
///
/// Coordinates: bottom-left origin, PDF points.
#[no_mangle]
pub unsafe extern "C" fn js_pdf_add_text(
    handle: i64,
    text_ptr: *const StringHeader,
    x: f64,
    y: f64,
    font_size: f64,
) {
    let text_handle = JsString::from_raw(text_ptr as *mut StringHeader);
    let text = match read_string(text_handle) {
        Some(s) => s.to_string(),
        None => return,
    };

    let size = if font_size > 0.0 {
        font_size as f32
    } else {
        DEFAULT_FONT_SIZE_PT
    };

    let mut guard = match state().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(doc) = guard.get_mut(&handle) else {
        warn_stale_handle_once(handle);
        return;
    };

    // Each text emission is a self-contained `BT ... ET` block: set
    // font, position the cursor, show the text, end the block. We
    // don't try to share font state across calls because that would
    // require tracking "current font size" in `OpenDoc` and re-
    // emitting the SetFont op only when it changes — overkill for the
    // v1 surface, which has no font selector beyond fontSize.
    let font = PdfFontHandle::Builtin(BuiltinFont::Helvetica);
    doc.current_ops.push(Op::StartTextSection);
    doc.current_ops.push(Op::SetFont {
        font,
        size: Pt(size),
    });
    doc.current_ops.push(Op::SetFillColor {
        col: Color::Rgb(Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            icc_profile: None,
        }),
    });
    doc.current_ops.push(Op::SetTextCursor {
        pos: Point {
            x: Pt(x as f32),
            y: Pt(y as f32),
        },
    });
    doc.current_ops.push(Op::ShowText {
        items: vec![TextItem::Text(text)],
    });
    doc.current_ops.push(Op::EndTextSection);
}

// ============================================================================
// FFI: pdfAddLine
// ============================================================================

/// `pdfAddLine(pdf, x1, y1, x2, y2)`.
///
/// Draws a single straight line in black with the default 1pt stroke
/// width. Coordinates: bottom-left origin, PDF points.
#[no_mangle]
pub unsafe extern "C" fn js_pdf_add_line(handle: i64, x1: f64, y1: f64, x2: f64, y2: f64) {
    let mut guard = match state().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(doc) = guard.get_mut(&handle) else {
        warn_stale_handle_once(handle);
        return;
    };

    let line = Line {
        points: vec![
            LinePoint {
                p: Point {
                    x: Pt(x1 as f32),
                    y: Pt(y1 as f32),
                },
                bezier: false,
            },
            LinePoint {
                p: Point {
                    x: Pt(x2 as f32),
                    y: Pt(y2 as f32),
                },
                bezier: false,
            },
        ],
        is_closed: false,
    };

    doc.current_ops.push(Op::SetOutlineColor {
        col: Color::Rgb(Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            icc_profile: None,
        }),
    });
    doc.current_ops
        .push(Op::SetOutlineThickness { pt: Pt(1.0) });
    doc.current_ops.push(Op::DrawLine { line });
}

// ============================================================================
// FFI: pdfNewPage
// ============================================================================

/// `pdfNewPage(pdf)` — finalize the current page and start a fresh one
/// with the same dimensions. No-op when the current page has no ops
/// yet (so calling `createPdf` immediately followed by `pdfNewPage`
/// doesn't emit a blank page).
#[no_mangle]
pub unsafe extern "C" fn js_pdf_new_page(handle: i64) {
    let mut guard = match state().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(doc) = guard.get_mut(&handle) else {
        warn_stale_handle_once(handle);
        return;
    };
    doc.finalize_current_page();
}

// ============================================================================
// FFI: pdfSave
// ============================================================================

/// `pdfSave(pdf)` — flush the current page, serialize the document,
/// write the file at the path passed to `createPdf`, and drop the
/// handle from the table. Errors (lock poisoning, I/O failure,
/// serialize warnings) are logged but not raised — the v1 surface
/// returns `void`, so failure modes are best-effort observable via
/// stderr.
#[no_mangle]
pub unsafe extern "C" fn js_pdf_save(handle: i64) {
    let mut doc = {
        let mut guard = match state().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.remove(&handle) {
            Some(d) => d,
            None => {
                warn_stale_handle_once(handle);
                return;
            }
        }
    };

    doc.finalize_current_page();

    let mut pdf = PdfDocument::new("Perry PDF");
    pdf.with_pages(doc.finished_pages);

    let mut warnings: Vec<PdfWarnMsg> = Vec::new();
    let bytes = pdf.save(&PdfSaveOptions::default(), &mut warnings);
    for w in &warnings {
        eprintln!("[perry-ext-pdf] save warning: {:?}", w);
    }

    if let Err(e) = fs::write(&doc.path, &bytes) {
        eprintln!("[perry-ext-pdf] failed to write PDF to {}: {}", doc.path, e);
    }
}

// ============================================================================
// Tiny JSON helper
// ============================================================================
//
// `perry-ffi::json_stringify` hands us a JSON string. To avoid pulling
// `serde_json` (and transitively `serde_derive` + proc-macros) into
// this small crate, parse just the two fields we need by hand. The
// shape is well-known and the input always comes from the JS side, so
// we don't have to handle arbitrary user JSON — only what a Perry
// JS-to-JSON serializer would emit for a plain object literal.

mod serde_json_lite {
    /// Extremely narrow JSON parser — only enough to read a flat
    /// object whose values are strings or numbers. Everything else
    /// (`null`, nested objects, arrays, booleans on a field we don't
    /// touch) is skipped silently. The full grammar is in
    /// `perry-stdlib`'s JSON parser; that's the right thing to use
    /// when this crate ever needs more.
    pub struct Value {
        fields: Vec<(String, Field)>,
    }
    enum Field {
        Str(String),
        Num(f64),
        Other,
    }

    #[allow(unused_assignments)]
    pub fn parse(input: &str) -> Option<Value> {
        let bytes = input.as_bytes();
        let mut i = 0usize;
        skip_ws(bytes, &mut i);
        if bytes.get(i).copied() != Some(b'{') {
            return None;
        }
        i += 1;
        let mut fields = Vec::new();
        loop {
            skip_ws(bytes, &mut i);
            if bytes.get(i).copied() == Some(b'}') {
                i += 1;
                break;
            }
            let key = parse_string(bytes, &mut i)?;
            skip_ws(bytes, &mut i);
            if bytes.get(i).copied() != Some(b':') {
                return None;
            }
            i += 1;
            skip_ws(bytes, &mut i);
            let value = parse_value(bytes, &mut i)?;
            fields.push((key, value));
            skip_ws(bytes, &mut i);
            if bytes.get(i).copied() == Some(b',') {
                i += 1;
                continue;
            }
            if bytes.get(i).copied() == Some(b'}') {
                i += 1;
                break;
            }
            return None;
        }
        Some(Value { fields })
    }

    impl Value {
        pub fn get_str(&self, key: &str) -> Option<&str> {
            for (k, v) in &self.fields {
                if k == key {
                    if let Field::Str(s) = v {
                        return Some(s);
                    }
                }
            }
            None
        }
        pub fn get_f32(&self, key: &str) -> Option<f32> {
            for (k, v) in &self.fields {
                if k == key {
                    if let Field::Num(n) = v {
                        return Some(*n as f32);
                    }
                }
            }
            None
        }
    }

    fn skip_ws(bytes: &[u8], i: &mut usize) {
        while let Some(&b) = bytes.get(*i) {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                *i += 1;
            } else {
                break;
            }
        }
    }

    fn parse_string(bytes: &[u8], i: &mut usize) -> Option<String> {
        if bytes.get(*i).copied() != Some(b'"') {
            return None;
        }
        *i += 1;
        let mut out = String::new();
        while let Some(&b) = bytes.get(*i) {
            match b {
                b'"' => {
                    *i += 1;
                    return Some(out);
                }
                b'\\' => {
                    *i += 1;
                    let esc = bytes.get(*i).copied()?;
                    *i += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\x08'),
                        b'f' => out.push('\x0c'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            // Skip 4 hex digits, decode as one Unicode
                            // BMP codepoint. Surrogate pairs are not
                            // handled — the v1 options object only
                            // contains paths and numbers, which never
                            // need surrogates.
                            let mut code: u32 = 0;
                            for _ in 0..4 {
                                let h = bytes.get(*i).copied()?;
                                *i += 1;
                                code = (code << 4)
                                    | match h {
                                        b'0'..=b'9' => (h - b'0') as u32,
                                        b'a'..=b'f' => (h - b'a' + 10) as u32,
                                        b'A'..=b'F' => (h - b'A' + 10) as u32,
                                        _ => return None,
                                    };
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => out.push(esc as char),
                    }
                }
                _ => {
                    out.push(b as char);
                    *i += 1;
                }
            }
        }
        None
    }

    fn parse_value(bytes: &[u8], i: &mut usize) -> Option<Field> {
        let start = bytes.get(*i).copied()?;
        if start == b'"' {
            return Some(Field::Str(parse_string(bytes, i)?));
        }
        if start == b'-' || start.is_ascii_digit() {
            let begin = *i;
            while let Some(&b) = bytes.get(*i) {
                if matches!(b, b'-' | b'+' | b'0'..=b'9' | b'.' | b'e' | b'E') {
                    *i += 1;
                } else {
                    break;
                }
            }
            let slice = std::str::from_utf8(&bytes[begin..*i]).ok()?;
            let n: f64 = slice.parse().ok()?;
            return Some(Field::Num(n));
        }
        // Skip null / true / false / nested objects / arrays without
        // capturing them — we never read those fields.
        match start {
            b't' | b'f' => {
                while let Some(&b) = bytes.get(*i) {
                    if b.is_ascii_alphabetic() {
                        *i += 1;
                    } else {
                        break;
                    }
                }
            }
            b'n' => {
                while let Some(&b) = bytes.get(*i) {
                    if b.is_ascii_alphabetic() {
                        *i += 1;
                    } else {
                        break;
                    }
                }
            }
            b'{' => {
                // Walk balanced braces.
                let mut depth = 0i32;
                while let Some(&b) = bytes.get(*i) {
                    *i += 1;
                    if b == b'{' {
                        depth += 1;
                    } else if b == b'}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if b == b'"' {
                        // Skip a string literal so braces inside it
                        // don't confuse the depth counter.
                        *i -= 1; // re-read the opening quote
                        let _ = parse_string(bytes, i)?;
                    }
                }
            }
            b'[' => {
                let mut depth = 0i32;
                while let Some(&b) = bytes.get(*i) {
                    *i += 1;
                    if b == b'[' {
                        depth += 1;
                    } else if b == b']' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if b == b'"' {
                        *i -= 1;
                        let _ = parse_string(bytes, i)?;
                    }
                }
            }
            _ => return None,
        }
        Some(Field::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::serde_json_lite::parse;

    #[test]
    fn parses_basic_options() {
        let v = parse(r#"{ "path": "/tmp/a.pdf", "pageWidth": 612, "pageHeight": 792.5 }"#)
            .expect("parse");
        assert_eq!(v.get_str("path"), Some("/tmp/a.pdf"));
        assert_eq!(v.get_f32("pageWidth"), Some(612.0));
        assert_eq!(v.get_f32("pageHeight"), Some(792.5));
    }

    #[test]
    fn missing_optional_returns_none() {
        let v = parse(r#"{ "path": "/tmp/b.pdf" }"#).expect("parse");
        assert_eq!(v.get_str("path"), Some("/tmp/b.pdf"));
        assert!(v.get_f32("pageWidth").is_none());
    }

    #[test]
    fn skips_unknown_value_kinds() {
        let v =
            parse(r#"{ "path": "/tmp/c.pdf", "meta": null, "tags": [1,2], "nested": {"x":1} }"#)
                .expect("parse");
        assert_eq!(v.get_str("path"), Some("/tmp/c.pdf"));
    }
}
