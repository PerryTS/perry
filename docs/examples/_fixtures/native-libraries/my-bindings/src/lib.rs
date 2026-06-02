//! Stub backing the `my-bindings` example in
//! `docs/src/native-libraries/authoring-guide.md`.
//!
//! This crate exists so the guide's TS surface (`src/index.ts`) and
//! consumer snippet (`test-app.ts`) point at a real, compileable
//! reference. The single `js_pdf_parse` export matches the FFI shape
//! the guide describes; the body is a deliberate stub (no upstream
//! `pdfium` dep) — its job is to drift-protect the `perry-ffi` API
//! the guide teaches against, not to actually parse PDFs.

use perry_ffi::{alloc_string, read_buffer_bytes, BufferHeader, StringHeader};

/// `pdf.parse(buf) -> string` — stub implementation.
///
/// # Safety
///
/// `buf_ptr` must be null or a Perry-runtime `BufferHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pdf_parse(buf_ptr: i64) -> *mut StringHeader {
    let buf_ptr = (buf_ptr as u64 & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader;
    let bytes = read_buffer_bytes(buf_ptr).unwrap_or(&[]);
    let summary = format!("stub: {} bytes", bytes.len());
    alloc_string(&summary).as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{alloc_buffer, read_string, JsString};

    #[test]
    fn parse_round_trip() {
        let buf = alloc_buffer(b"hello");
        let nan_boxed = (0x7FFD_u64 << 48) | (buf as u64 & 0x0000_FFFF_FFFF_FFFF);
        let out_ptr = unsafe { js_pdf_parse(nan_boxed as i64) };
        let out = read_string(unsafe { JsString::from_raw(out_ptr) }).expect("non-null");
        assert_eq!(out, "stub: 5 bytes");
    }

    #[test]
    fn parse_null_buffer() {
        let out_ptr = unsafe { js_pdf_parse(0) };
        let out = read_string(unsafe { JsString::from_raw(out_ptr) }).expect("non-null");
        assert_eq!(out, "stub: 0 bytes");
    }
}
