//! Issue #1210: `buffer.transcode(source, fromEnc, toEnc)`.
//!
//! Node's `node:buffer` module exports a `transcode` helper that
//! re-encodes the bytes of a Buffer from one supported encoding into
//! another. The Node implementation accepts a wide encoding matrix
//! (icu-backed); we land a deterministic subset that covers the
//! published Deno node-compat fixtures and the common JS-string-bridge
//! use cases without taking an icu dependency:
//!
//!   - `utf16le` / `ucs2` / `ucs-2` / `utf-16le` → `utf8` / `utf-8`
//!   - `utf8` / `utf-8` / `ascii` / `latin1` / `binary` → `utf16le` / `ucs2`
//!
//! Unsupported encodings/pairs throw Node's ICU-style
//! `U_ILLEGAL_ARGUMENT_ERROR`, while invalid sources throw
//! `ERR_INVALID_ARG_TYPE` for the `source` argument.

use super::*;

#[derive(Copy, Clone, PartialEq, Eq)]
enum TranscodeEnc {
    Utf8,
    Utf16Le,
    Latin1,
    Other,
}

fn classify_encoding(value: f64) -> TranscodeEnc {
    let str_ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return TranscodeEnc::Other;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        if len == 0 || len > 16 {
            return TranscodeEnc::Other;
        }
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        let mut lower = [0u8; 16];
        for (i, b) in bytes.iter().enumerate() {
            lower[i] = b.to_ascii_lowercase();
        }
        let lower = &lower[..len];
        match lower {
            b"utf8" | b"utf-8" => TranscodeEnc::Utf8,
            b"utf16le" | b"utf-16le" | b"ucs2" | b"ucs-2" => TranscodeEnc::Utf16Le,
            b"latin1" | b"binary" | b"ascii" => TranscodeEnc::Latin1,
            _ => TranscodeEnc::Other,
        }
    }
}

fn raw_addr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
        bits as usize
    } else {
        0
    }
}

fn describe_source(value: f64) -> String {
    let addr = raw_addr_from_value(value);
    if addr != 0 {
        if is_data_view(addr) {
            return "an instance of DataView".to_string();
        }
        if is_array_buffer(addr) {
            return "an instance of ArrayBuffer".to_string();
        }
        if is_shared_array_buffer(addr) {
            return "an instance of SharedArrayBuffer".to_string();
        }
    }
    crate::fs::validate::describe_received(value)
}

fn throw_invalid_source(value: f64) -> ! {
    let message = format!(
        "The \"source\" argument must be an instance of Buffer or Uint8Array. Received {}",
        describe_source(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_transcode_error() -> ! {
    let message = b"Unable to transcode Buffer [U_ILLEGAL_ARGUMENT_ERROR]";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "U_ILLEGAL_ARGUMENT_ERROR");
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn buffer_bytes<'a>(buf_ptr: *const BufferHeader) -> &'a [u8] {
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return &[];
    }
    unsafe {
        let len = (*buf_ptr).length as usize;
        let data = buffer_data(buf_ptr);
        std::slice::from_raw_parts(data, len)
    }
}

fn source_bytes(value: f64) -> &'static [u8] {
    let addr = raw_addr_from_value(value);
    if addr != 0 {
        if is_uint8array_buffer(addr)
            || (is_registered_buffer(addr) && !is_any_array_buffer(addr) && !is_data_view(addr))
        {
            return buffer_bytes(addr as *const BufferHeader);
        }
        if crate::typedarray::lookup_typed_array_kind(addr) == Some(crate::typedarray::KIND_UINT8) {
            let ptr = addr as *const crate::typedarray::TypedArrayHeader;
            if let Some(bytes) = unsafe { crate::typedarray::typed_array_bytes(ptr) } {
                return bytes;
            }
        }
    }
    throw_invalid_source(value)
}

fn buffer_from_bytes(out: &[u8]) -> *mut BufferHeader {
    let buf = buffer_alloc(out.len() as u32);
    unsafe {
        (*buf).length = out.len() as u32;
        if !out.is_empty() {
            std::ptr::copy_nonoverlapping(out.as_ptr(), buffer_data_mut(buf), out.len());
        }
    }
    buf
}

fn utf16le_to_utf8(bytes: &[u8]) -> Vec<u8> {
    // Walk u16 code units, lossy-decode (unpaired surrogates → U+FFFD)
    // to mirror Node/ICU's "lossy" default for the buffer.transcode
    // path. Odd-length input drops the trailing byte, matching Node.
    let chunks = bytes.chunks_exact(2);
    let u16_iter = chunks.map(|c| u16::from_le_bytes([c[0], c[1]]));
    let decoded: String = char::decode_utf16(u16_iter)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect();
    decoded.into_bytes()
}

fn utf8_to_utf16le(bytes: &[u8]) -> Vec<u8> {
    let cow = String::from_utf8_lossy(bytes);
    let mut out = Vec::with_capacity(cow.len() * 2);
    for unit in cow.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

fn latin1_to_utf16le(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.extend_from_slice(&(b as u16).to_le_bytes());
    }
    out
}

fn latin1_to_utf8(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b);
        } else {
            // Latin-1 maps 0x80..0xFF → U+0080..U+00FF, which is a
            // 2-byte UTF-8 sequence.
            out.push(0xC0 | (b >> 6));
            out.push(0x80 | (b & 0x3F));
        }
    }
    out
}

/// `buffer.transcode(source, fromEnc, toEnc)` — re-encode Buffer bytes.
///
/// `source_f64` is a NaN-boxed Buffer pointer; `from_enc_f64`/`to_enc_f64`
/// are NaN-boxed encoding-name strings.  Returns a freshly-allocated
/// Buffer holding the re-encoded bytes.  Unsupported encoding pairs throw.
#[no_mangle]
pub extern "C" fn js_buffer_transcode(
    source_f64: f64,
    from_enc_f64: f64,
    to_enc_f64: f64,
) -> *mut BufferHeader {
    let src_bytes = source_bytes(source_f64);
    let from = classify_encoding(from_enc_f64);
    let to = classify_encoding(to_enc_f64);
    if from == TranscodeEnc::Other || to == TranscodeEnc::Other {
        throw_transcode_error();
    }

    if from == to {
        return buffer_from_bytes(src_bytes);
    }

    let out: Vec<u8> = match (from, to) {
        (TranscodeEnc::Utf16Le, TranscodeEnc::Utf8) => utf16le_to_utf8(src_bytes),
        (TranscodeEnc::Utf8, TranscodeEnc::Utf16Le) => utf8_to_utf16le(src_bytes),
        (TranscodeEnc::Latin1, TranscodeEnc::Utf16Le) => latin1_to_utf16le(src_bytes),
        (TranscodeEnc::Latin1, TranscodeEnc::Utf8) => latin1_to_utf8(src_bytes),
        // utf8/utf16le → latin1: lossy narrow per Node — only the low byte
        // of each code unit is kept; code points > 0xFF emit '?'.
        (TranscodeEnc::Utf16Le, TranscodeEnc::Latin1) => {
            let chunks = src_bytes.chunks_exact(2);
            let mut out = Vec::with_capacity(chunks.len());
            for c in chunks {
                let unit = u16::from_le_bytes([c[0], c[1]]);
                out.push(if unit > 0xFF { b'?' } else { unit as u8 });
            }
            out
        }
        (TranscodeEnc::Utf8, TranscodeEnc::Latin1) => {
            let cow = String::from_utf8_lossy(src_bytes);
            let mut out = Vec::with_capacity(cow.len());
            for ch in cow.chars() {
                let cp = ch as u32;
                out.push(if cp > 0xFF { b'?' } else { cp as u8 });
            }
            out
        }
        _ => throw_transcode_error(),
    };

    buffer_from_bytes(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16le_to_utf8_basic() {
        // "Hi" as UTF-16LE: H=0x48 0x00, i=0x69 0x00
        let bytes = [0x48u8, 0x00, 0x69, 0x00];
        let out = utf16le_to_utf8(&bytes);
        assert_eq!(out, b"Hi");
    }

    #[test]
    fn utf16le_to_utf8_odd_length_drops_trailing() {
        let bytes = [0x48u8, 0x00, 0x69];
        let out = utf16le_to_utf8(&bytes);
        assert_eq!(out, b"H");
    }

    #[test]
    fn utf8_to_utf16le_basic() {
        let out = utf8_to_utf16le(b"Hi");
        assert_eq!(out, [0x48, 0x00, 0x69, 0x00]);
    }

    #[test]
    fn latin1_to_utf8_high_byte() {
        // 0xE9 = 'é' in Latin-1 → 0xC3 0xA9 in UTF-8.
        let out = latin1_to_utf8(&[0xE9]);
        assert_eq!(out, [0xC3, 0xA9]);
    }
}
