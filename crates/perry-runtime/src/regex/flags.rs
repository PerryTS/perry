//! RegExp flags validation (`RegExpInitialize`, #2829).
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use super::throw_regexp_syntax_error;

/// #2829: validate a RegExp flags string the way the spec's
/// `RegExpInitialize` does — each flag must be one of `dgimsuvy` and must not
/// repeat. Returns the flags in canonical (sorted) order, or throws a
/// `SyntaxError` mirroring Node's "Invalid flags supplied to RegExp
/// constructor '<flags>'" message.
///
/// Note: the `v` flag (unicodeSets) is accepted as a valid flag for parity but
/// its set-notation matching semantics are not implemented (the regex crate
/// has no equivalent); it behaves like an ordinary unicode pattern.
#[cfg(feature = "regex-engine")]
/// The canonical flag text, held inline.
///
/// There are eight legal flags and each may appear once, so the canonical form
/// is at most eight ASCII bytes and never needs the heap. It used to be a
/// `String`, i.e. one heap allocation on **every** `RegExp` construction — and
/// a JS regex literal constructs a fresh object every time it is evaluated, so
/// on the claude-code TUI that was ~162,000 allocations per 400-character
/// reply for text that is almost always one or two bytes.
#[derive(Clone, Copy)]
pub(super) struct CanonicalFlags {
    buf: [u8; 8],
    len: u8,
}

impl CanonicalFlags {
    pub(super) fn as_str(&self) -> &str {
        // Every byte written below comes from `FLAG_ORDER`, which is ASCII.
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

pub(super) fn validate_and_canonicalize_flags(flags: &str) -> CanonicalFlags {
    // Spec order of the flag bits: d g i m s u v y.
    const FLAG_ORDER: &[char] = &['d', 'g', 'i', 'm', 's', 'u', 'v', 'y'];
    let mut seen = [false; 8];
    for ch in flags.chars() {
        match FLAG_ORDER.iter().position(|&f| f == ch) {
            Some(idx) => {
                if seen[idx] {
                    throw_regexp_syntax_error(&format!(
                        "Invalid flags supplied to RegExp constructor '{}'",
                        flags
                    ));
                }
                seen[idx] = true;
            }
            None => {
                throw_regexp_syntax_error(&format!(
                    "Invalid flags supplied to RegExp constructor '{}'",
                    flags
                ));
            }
        }
    }
    let mut out = CanonicalFlags {
        buf: [0; 8],
        len: 0,
    };
    for (i, c) in FLAG_ORDER.iter().enumerate() {
        if seen[i] {
            out.buf[out.len as usize] = *c as u8;
            out.len += 1;
        }
    }
    out
}
