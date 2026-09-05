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
pub(super) fn validate_and_canonicalize_flags(flags: &str) -> String {
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
    FLAG_ORDER
        .iter()
        .enumerate()
        .filter(|(i, _)| seen[*i])
        .map(|(_, c)| *c)
        .collect()
}
