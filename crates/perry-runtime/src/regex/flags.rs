//! RegExp flags validation (`RegExpInitialize`, #2829).
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use super::throw_regexp_syntax_error;

/// The canonical flags held inline. Validation borrows only the original
/// bytes; no heap allocation or JS throw occurs inside that borrowed view.
#[derive(Clone, Copy)]
pub(crate) struct CanonicalFlags {
    buf: [u8; 8],
    len: u8,
}

impl CanonicalFlags {
    /// Validate borrowed flag bytes without allocating or throwing inside a
    /// moving-string view. The owned result is safe across collector polls.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        const ORDER: &[u8] = b"dgimsuvy";
        if bytes.len() > ORDER.len() {
            return None;
        }
        let mut seen = 0u8;
        for byte in bytes {
            let bit = 1 << ORDER.iter().position(|flag| flag == byte)?;
            if seen & bit != 0 {
                return None;
            }
            seen |= bit;
        }
        if seen & (1 << 5) != 0 && seen & (1 << 6) != 0 {
            return None;
        }
        let mut out = Self {
            buf: [0; 8],
            len: 0,
        };
        for (index, &flag) in ORDER.iter().enumerate() {
            if seen & (1 << index) != 0 {
                out.buf[out.len as usize] = flag;
                out.len += 1;
            }
        }
        Some(out)
    }

    pub(super) fn as_str(&self) -> &str {
        // Every stored byte comes from the canonical flag order, which is ASCII.
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

pub(crate) fn validate_and_canonicalize_flags(flags: &str) -> CanonicalFlags {
    CanonicalFlags::parse(flags.as_bytes()).unwrap_or_else(|| {
        throw_regexp_syntax_error("Invalid flags supplied to RegExp constructor")
    })
}
