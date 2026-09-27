//! Handle-free `createHash` / `createHmac` chains (#11516).
//!
//! `perry_transform::crypto_hash_chain` proves that a hash or HMAC object
//! never escapes: it is a pure `crypto.createHash(a).update(x).digest(e)`
//! chain, or a block-local `const h = crypto.createHash(a)` whose only uses
//! are `h.update(...)` / `h.digest(...)` in the same frame. Codegen then gives
//! each such site a stack slot of [`CHAIN_STATE_BYTES`] bytes, and these entry
//! points keep the digest state in that slot instead of in the common handle
//! registry.
//!
//! Nothing is registered, so no handle id is consumed, parked, or proven dead
//! by a full trace (#11515's reclamation cost), and nothing needs freeing: the
//! state holds no heap memory, and an exception that abandons the chain just
//! leaves dead bytes in a frame that is unwinding anyway.
//!
//! Semantics are the handle path's, by construction: state creation, update
//! decoding and digest encoding are the same functions `dispatch_hash` /
//! `dispatch_hmac` call. A second `digest()` or an `update()` after `digest()`
//! still observes the finalized state and throws `ERR_CRYPTO_HASH_FINALIZED`
//! (an HMAC's second `digest()` returns an empty value), exactly as on a
//! registered handle.

use super::hash_handles::{
    hash_digest_value, hash_update_bytes, hmac_digest_value, latin1_string,
    new_hash_state_or_throw, new_hmac_state_or_throw, update_hash_state, update_hmac_state,
    HashState, HmacState,
};
use super::*;

/// Size of the per-site stack slot codegen reserves for a chain state.
/// Mirrored by `perry-codegen`'s `CRYPTO_CHAIN_STATE_BYTES`; the const
/// assertions below fail the build if the state outgrows it.
pub const CHAIN_STATE_BYTES: usize = 1024;
/// Alignment of that slot (`u128` block counters in SHA-384/512 need 16).
pub const CHAIN_STATE_ALIGN: usize = 16;

enum ChainState {
    Hash {
        /// `None` once `digest()` consumed it.
        state: Option<HashState>,
        output_len: Option<usize>,
    },
    Hmac {
        state: Option<HmacState>,
    },
}

const _: () = assert!(std::mem::size_of::<ChainState>() <= CHAIN_STATE_BYTES);
const _: () = assert!(std::mem::align_of::<ChainState>() <= CHAIN_STATE_ALIGN);

#[inline]
unsafe fn chain_state<'a>(state: f64) -> &'a mut ChainState {
    &mut *(state.to_bits() as usize as *mut ChainState)
}

#[inline]
fn chain_value(slot: *mut u8) -> f64 {
    // A raw frame address: its top 16 bits are zero, so it reads as a plain
    // (subnormal) number and never as a NaN-boxed heap reference.
    f64::from_bits(slot as u64)
}

fn throw_finalized() -> ! {
    perry_runtime::fs::validate::throw_error_with_code(
        "Digest already called",
        "ERR_CRYPTO_HASH_FINALIZED",
    )
}

/// `crypto.createHash(alg, options)` into the frame slot `slot`. Returns the
/// opaque state value the chain's `update`/`digest` calls take.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_chain_hash_init(
    slot: *mut u8,
    alg_ptr: i64,
    options: f64,
) -> f64 {
    let (state, output_len) = new_hash_state_or_throw(alg_ptr, options);
    std::ptr::write(
        slot as *mut ChainState,
        ChainState::Hash {
            state: Some(state),
            output_len,
        },
    );
    chain_value(slot)
}

/// `crypto.createHmac(alg, key)` into the frame slot `slot`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_chain_hmac_init(
    slot: *mut u8,
    alg_ptr: i64,
    key_ptr: i64,
) -> f64 {
    let state = new_hmac_state_or_throw(alg_ptr, key_ptr);
    std::ptr::write(
        slot as *mut ChainState,
        ChainState::Hmac { state: Some(state) },
    );
    chain_value(slot)
}

/// `.update(data, inputEncoding)`; `inputEncoding` is `undefined` when the
/// call site passed none. Returns `state`, like `update` returns `this`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_chain_update(state: f64, data: f64, encoding: f64) -> f64 {
    match chain_state(state) {
        ChainState::Hash { state: s, .. } => {
            let Some(s) = s.as_mut() else {
                throw_finalized()
            };
            let bytes = hash_update_bytes(&[data, encoding]);
            update_hash_state(s, &bytes);
        }
        ChainState::Hmac { state: s } => {
            let Some(s) = s.as_mut() else {
                throw_finalized()
            };
            let bytes = hash_update_bytes(&[data, encoding]);
            update_hmac_state(s, &bytes);
        }
    }
    state
}

/// `.digest(outputEncoding)`; `outputEncoding` is `undefined` when the call
/// site passed none (a Buffer result).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_chain_digest(state: f64, encoding: f64) -> f64 {
    match chain_state(state) {
        ChainState::Hash {
            state: s,
            output_len,
        } => {
            if s.is_none() {
                throw_finalized();
            }
            hash_digest_value(s.take(), *output_len, Some(encoding))
        }
        ChainState::Hmac { state: s } => hmac_digest_value(s.take(), Some(encoding)),
    }
}

/// How `digest(outputEncoding)` renders its bytes. Mirrors node's
/// `outputEncoding && \`${outputEncoding}\``, then `ParseEncoding(..., BUFFER)`:
/// a missing, falsy or unrecognised encoding yields a `Buffer`.
///
/// Perry extension kept from the handle path: an options object (what
/// `crypto.hash(alg, data, { outputEncoding, outputLength })` lowers to)
/// supplies `outputEncoding` (default `hex`) and the XOF `outputLength`.
pub(super) struct DigestEncoding {
    kind: DigestKind,
    pub(super) output_len: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DigestKind {
    Buffer,
    Hex,
    Base64,
    Base64Url,
    Latin1,
    Utf8,
    Utf16le,
    Ascii,
}

fn digest_kind(name: &str) -> DigestKind {
    match name.to_ascii_lowercase().as_str() {
        "hex" => DigestKind::Hex,
        "base64" => DigestKind::Base64,
        "base64url" => DigestKind::Base64Url,
        "latin1" | "binary" => DigestKind::Latin1,
        "utf8" | "utf-8" => DigestKind::Utf8,
        "ucs2" | "ucs-2" | "utf16le" | "utf-16le" => DigestKind::Utf16le,
        "ascii" => DigestKind::Ascii,
        _ => DigestKind::Buffer,
    }
}

impl DigestEncoding {
    pub(super) unsafe fn parse(arg: Option<f64>) -> Self {
        let buffer = DigestEncoding {
            kind: DigestKind::Buffer,
            output_len: None,
        };
        let Some(arg) = arg else {
            return buffer;
        };
        let bits = arg.to_bits();
        if let Some(name) = string_from_jsvalue(bits) {
            return DigestEncoding {
                kind: digest_kind(&name),
                output_len: None,
            };
        }
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if (bits >> 48) as u16 == 0x7FFD
            && addr >= 0x1000
            && !perry_runtime::buffer::is_registered_buffer(addr)
        {
            let output_len = object_field_bits(bits, b"outputLength")
                .and_then(|b| nanboxed_to_usize(f64::from_bits(b)));
            let kind = object_field_string(bits, b"outputEncoding")
                .map(|name| digest_kind(&name))
                .unwrap_or(DigestKind::Hex);
            return DigestEncoding { kind, output_len };
        }
        buffer
    }

    pub(super) unsafe fn output(&self, bytes: &[u8]) -> f64 {
        let text = match self.kind {
            DigestKind::Buffer => {
                let buf = alloc_buffer_from_slice(bytes);
                return f64::from_bits(
                    0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                );
            }
            DigestKind::Utf16le => {
                let wtf8 = utf16le_to_wtf8(bytes);
                let s = perry_runtime::string::js_string_from_wtf8_bytes(
                    wtf8.as_ptr(),
                    wtf8.len() as u32,
                );
                return f64::from_bits(
                    0x7FFF_0000_0000_0000u64 | ((s as u64) & 0x0000_FFFF_FFFF_FFFF),
                );
            }
            DigestKind::Hex => perry_hex::encode(bytes),
            DigestKind::Base64 => perry_base64::engine::general_purpose::STANDARD.encode(bytes),
            DigestKind::Base64Url => {
                perry_base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
            }
            DigestKind::Latin1 => latin1_string(bytes),
            DigestKind::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            // node's ASCII decode clears the high bit of every byte.
            DigestKind::Ascii => bytes.iter().map(|&b| char::from(b & 0x7F)).collect(),
        };
        let s = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        f64::from_bits(0x7FFF_0000_0000_0000u64 | ((s as u64) & 0x0000_FFFF_FFFF_FFFF))
    }
}

/// Little-endian UTF-16 code units to WTF-8: a lone surrogate survives as its
/// three-byte generalized-UTF-8 form, as it does in a JS string. A trailing odd
/// byte is dropped, like `Buffer#toString('utf16le')`.
fn utf16le_to_wtf8(bytes: &[u8]) -> Vec<u8> {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
    let mut out = Vec::with_capacity(bytes.len() * 3 / 2);
    for decoded in char::decode_utf16(units) {
        match decoded {
            Ok(c) => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            Err(e) => {
                let u = e.unpaired_surrogate();
                out.push(0xE0 | (u >> 12) as u8);
                out.push(0x80 | ((u >> 6) & 0x3F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_kind_matches_node_parse_encoding() {
        assert_eq!(digest_kind("HEX"), DigestKind::Hex);
        assert_eq!(digest_kind("UTF-8"), DigestKind::Utf8);
        assert_eq!(digest_kind("ucs-2"), DigestKind::Utf16le);
        assert_eq!(digest_kind("binary"), DigestKind::Latin1);
        // node: an unknown encoding falls back to BUFFER.
        assert_eq!(digest_kind("nope"), DigestKind::Buffer);
        assert_eq!(digest_kind(""), DigestKind::Buffer);
        assert_eq!(digest_kind("buffer"), DigestKind::Buffer);
    }

    #[test]
    fn utf16le_keeps_lone_surrogates_and_drops_odd_byte() {
        // U+0041, lone high surrogate U+D800, then an odd trailing byte.
        let out = utf16le_to_wtf8(&[0x41, 0x00, 0x00, 0xD8, 0x7F]);
        assert_eq!(out, vec![0x41, 0xED, 0xA0, 0x80]);
        // A valid pair decodes to one supplementary code point.
        let pair = utf16le_to_wtf8(&[0x3D, 0xD8, 0x00, 0xDE]);
        assert_eq!(pair, "\u{1F600}".as_bytes());
    }

    #[test]
    fn chain_state_fits_the_codegen_slot() {
        assert!(std::mem::size_of::<ChainState>() <= CHAIN_STATE_BYTES);
        assert!(std::mem::align_of::<ChainState>() <= CHAIN_STATE_ALIGN);
    }
}
