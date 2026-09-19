//! WTF-8 payload borrows for the paths that need Unicode *semantics* —
//! normalization, case mapping and collation — rather than raw bytes.
//!
//! Perry stores strings as WTF-8, so a heap payload may contain a lone
//! surrogate encoded as three bytes (`ED A0..BF 80..BF`). Those bytes are
//! **not** valid UTF-8, so `str::from_utf8_unchecked` over them is undefined
//! behaviour: the decoded `char` would sit in `U+D800..=U+DFFF`, a range
//! Rust's `char` is documented never to hold.
//!
//! [`Wtf8Str`] is the borrow that makes that impossible to get wrong. It pairs
//! the payload bytes with the header's `STRING_FLAG_HAS_LONE_SURROGATES` bit —
//! the same guard `js_string_is_well_formed` / `js_string_to_well_formed`
//! already use — and only hands out a `&str` when that bit is clear. Everything
//! else goes through [`Wtf8Str::code_points`] (a `u32` walk that can represent
//! a surrogate) or through the run-splitting transforms below, which apply a
//! `&str` operation to each maximal well-formed run and pass lone surrogates
//! through untouched.
//!
//! Splitting at surrogate boundaries is not an approximation for normalization
//! and case mapping: a lone surrogate is unassigned, so it has canonical
//! combining class 0 (a starter, which blocks composition and stops canonical
//! reordering), no decomposition mapping, and is neither `Cased` nor
//! `Case_Ignorable` (so it breaks the Final_Sigma context). Normalizing the
//! runs independently is therefore exactly what a code-point-level normalizer
//! does, which `wtf8_normalize_matches_the_node_oracle` pins against V8/ICU
//! output. (#10692)

use super::*;

/// A borrowed string payload that may be WTF-8.
///
/// Cheap to copy; the borrow follows the same rule as
/// [`crate::string::string_as_str`] — it must not outlive any call that can
/// move the payload.
#[derive(Clone, Copy)]
pub(crate) struct Wtf8Str<'a> {
    bytes: &'a [u8],
    /// The header's `STRING_FLAG_HAS_LONE_SURROGATES` bit.
    lone: bool,
}

impl<'a> Wtf8Str<'a> {
    /// Borrow a Rust `&str`. Valid UTF-8 by construction, so never WTF-8.
    ///
    /// Only the NFC re-borrow in `locale_compare_canonical` needs this in a
    /// product build, and that is behind `string-normalize`; without the
    /// feature it would be dead code, which `-D warnings` rejects.
    #[cfg(any(test, feature = "string-normalize"))]
    #[inline]
    pub(crate) fn from_str(s: &'a str) -> Self {
        Wtf8Str {
            bytes: s.as_bytes(),
            lone: false,
        }
    }

    /// Borrow bytes that are known to be a WTF-8 payload with lone surrogates.
    /// Used for the intermediate buffers the run-splitting transforms produce
    /// — which, like [`Wtf8Str::from_str`], only exist under
    /// `string-normalize` outside of tests.
    #[cfg(any(test, feature = "string-normalize"))]
    #[inline]
    pub(crate) fn from_wtf8_bytes(bytes: &'a [u8]) -> Self {
        Wtf8Str { bytes, lone: true }
    }

    /// Borrow a live string header's payload.
    ///
    /// # Safety
    /// `s` must point at a live `StringHeader`, and the borrow must not
    /// outlive any call that can move it.
    #[inline]
    pub(crate) unsafe fn from_header(s: *const StringHeader) -> Self {
        let len = (*s).byte_len as usize;
        Wtf8Str {
            bytes: slice::from_raw_parts(string_data(s), len),
            lone: (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0,
        }
    }

    #[inline]
    pub(crate) fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The payload as a `&str`, or `None` when it carries lone surrogates.
    ///
    /// This is the guard the whole module exists for: the flag is set at
    /// construction by the same WTF-8 walk that computes `utf16_len`, so a
    /// clear flag means the payload decoded without a surrogate code point
    /// and `from_utf8_unchecked` is justified. `js_string_is_well_formed` and
    /// `js_string_to_well_formed` already rely on exactly this bit.
    #[inline]
    pub(crate) fn as_str(&self) -> Option<&'a str> {
        if self.lone {
            return None;
        }
        debug_assert!(
            str::from_utf8(self.bytes).is_ok(),
            "STRING_FLAG_HAS_LONE_SURROGATES clear but payload is not valid UTF-8"
        );
        // SAFETY: see the doc comment — the cleared flag proves valid UTF-8.
        Some(unsafe { str::from_utf8_unchecked(self.bytes) })
    }

    /// Re-borrow from `offset`, which must be a code-point boundary.
    #[inline]
    pub(crate) fn slice_from(&self, offset: usize) -> Wtf8Str<'a> {
        Wtf8Str {
            bytes: &self.bytes[offset..],
            lone: self.lone,
        }
    }

    /// Walk the payload as Unicode code points. Unlike `str::chars` this can
    /// yield a value in `U+D800..=U+DFFF`, which is the whole point.
    #[inline]
    pub(crate) fn code_points(&self) -> CodePoints<'a> {
        match self.as_str() {
            Some(s) => CodePoints::Utf8(s.chars()),
            None => CodePoints::Wtf8 {
                bytes: self.bytes,
                offset: 0,
            },
        }
    }
}

/// Code-point walk over a possibly-WTF-8 payload.
///
/// The well-formed arm keeps `str::Chars` so the hot path decodes with the
/// same core routine (and the same codegen) it always has.
pub(crate) enum CodePoints<'a> {
    Utf8(str::Chars<'a>),
    Wtf8 { bytes: &'a [u8], offset: usize },
}

impl Iterator for CodePoints<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        match self {
            CodePoints::Utf8(chars) => chars.next().map(u32::from),
            CodePoints::Wtf8 { bytes, offset } => {
                if *offset >= bytes.len() {
                    return None;
                }
                let (advance, _units, code_point) = wtf8_step(bytes, *offset);
                *offset = (*offset + advance).min(bytes.len());
                Some(code_point)
            }
        }
    }
}

/// `char::is_lowercase` lifted to a code point. A lone surrogate is
/// unassigned, hence uncased — which is also what `char::is_lowercase`
/// answered for the (illegally constructed) surrogate `char` this replaces.
#[inline]
pub(crate) fn code_point_is_lowercase(code_point: u32) -> bool {
    char::from_u32(code_point).is_some_and(char::is_lowercase)
}

/// The first scalar of `char::to_lowercase`, lifted to a code point. A lone
/// surrogate has no case mapping, so it maps to itself.
#[inline]
pub(crate) fn code_point_to_lowercase_first(code_point: u32) -> u32 {
    match char::from_u32(code_point) {
        Some(c) => c.to_lowercase().next().map_or(code_point, u32::from),
        None => code_point,
    }
}

/// The four Unicode normalization forms `String.prototype.normalize` accepts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NormalizeForm {
    Nfc,
    Nfd,
    Nfkc,
    Nfkd,
}

impl NormalizeForm {
    /// Parse the (already `ToString`-coerced) `form` argument. `None` is the
    /// caller's signal to throw the spec `RangeError`.
    pub(crate) fn parse(form: &str) -> Option<NormalizeForm> {
        match form {
            "NFC" => Some(NormalizeForm::Nfc),
            "NFD" => Some(NormalizeForm::Nfd),
            "NFKC" => Some(NormalizeForm::Nfkc),
            "NFKD" => Some(NormalizeForm::Nfkd),
            _ => None,
        }
    }
}

#[cfg(feature = "string-normalize")]
fn normalize_run(run: &str, form: NormalizeForm) -> String {
    use unicode_normalization::UnicodeNormalization;
    match form {
        NormalizeForm::Nfc => run.nfc().collect(),
        NormalizeForm::Nfd => run.nfd().collect(),
        NormalizeForm::Nfkc => run.nfkc().collect(),
        NormalizeForm::Nfkd => run.nfkd().collect(),
    }
}

/// Normalize engine gated off: the four valid forms pass the text through
/// unchanged (no Unicode decomposition tables linked). Form *validity* is
/// still enforced by the caller, so a bad form throws either way.
#[cfg(not(feature = "string-normalize"))]
fn normalize_run(run: &str, _form: NormalizeForm) -> String {
    run.to_string()
}

/// Unicode-normalize a payload, preserving lone surrogates verbatim.
pub(crate) fn normalize(subject: Wtf8Str<'_>, form: NormalizeForm) -> Vec<u8> {
    match subject.as_str() {
        Some(s) => normalize_run(s, form).into_bytes(),
        None => transform_runs(subject.bytes(), &mut |run, out| {
            out.extend_from_slice(normalize_run(run, form).as_bytes())
        }),
    }
}

/// `str::to_lowercase` over a payload, preserving lone surrogates verbatim.
///
/// The result is compared as bytes, which is the same order as comparing the
/// lowercased code-point sequences: WTF-8 byte order and code-point order
/// agree (a lone surrogate encodes as `ED A0..BF ..`, between `U+D7FF`'s
/// `ED 80..9F ..` and `U+E000`'s `EE ..`).
pub(crate) fn to_lowercase(subject: Wtf8Str<'_>) -> Vec<u8> {
    match subject.as_str() {
        Some(s) => s.to_lowercase().into_bytes(),
        None => transform_runs(subject.bytes(), &mut |run, out| {
            out.extend_from_slice(run.to_lowercase().as_bytes())
        }),
    }
}

/// Rebuild a WTF-8 payload by applying `transform` to each maximal
/// lone-surrogate-free run and copying every lone surrogate through unchanged.
///
/// Runs are borrowed with the **checked** `str::from_utf8`: a run of a
/// well-formed WTF-8 payload is valid UTF-8, but this path is rare enough that
/// paying for the proof is cheaper than assuming it, and a malformed payload
/// (a truncated lead from an FFI blob, #6085) then degrades to a verbatim copy
/// instead of undefined behaviour.
fn transform_runs(bytes: &[u8], transform: &mut dyn FnMut(&str, &mut Vec<u8>)) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut run_start = 0usize;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let (advance, units, code_point) = wtf8_step(bytes, offset);
        let next = (offset + advance).min(bytes.len());
        if units == 1 && (0xD800..=0xDFFF).contains(&code_point) {
            flush_run(&bytes[run_start..offset], transform, &mut out);
            out.extend_from_slice(&bytes[offset..next]);
            run_start = next;
        }
        offset = next;
    }
    flush_run(&bytes[run_start..], transform, &mut out);
    out
}

fn flush_run(run: &[u8], transform: &mut dyn FnMut(&str, &mut Vec<u8>), out: &mut Vec<u8>) {
    if run.is_empty() {
        return;
    }
    match str::from_utf8(run) {
        Ok(s) => transform(s, out),
        // Not reachable for a well-formed payload; copying verbatim keeps a
        // malformed one out of `from_utf8_unchecked`.
        Err(_) => out.extend_from_slice(run),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a UTF-16 code-unit sequence as WTF-8, the way a Perry payload
    /// built from `"\uD800"` holds it.
    fn wtf8(units: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        for &unit in units {
            let code_point = unit as u32;
            if code_point < 0x80 {
                out.push(code_point as u8);
            } else if code_point < 0x800 {
                out.push(0xC0 | (code_point >> 6) as u8);
                out.push(0x80 | (code_point & 0x3F) as u8);
            } else {
                out.push(0xE0 | (code_point >> 12) as u8);
                out.push(0x80 | ((code_point >> 6) & 0x3F) as u8);
                out.push(0x80 | (code_point & 0x3F) as u8);
            }
        }
        out
    }

    /// Decode WTF-8 back to UTF-16 code units for comparison with the oracle
    /// table, which is written in the `charCodeAt` terms JavaScript sees.
    fn units(bytes: &[u8]) -> Vec<u16> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        while offset < bytes.len() {
            let (advance, count, code_point) = wtf8_step(bytes, offset);
            if count == 2 {
                let v = code_point - 0x1_0000;
                out.push(0xD800 + (v >> 10) as u16);
                out.push(0xDC00 + (v & 0x3FF) as u16);
            } else {
                out.push(code_point as u16);
            }
            offset = (offset + advance).min(bytes.len());
        }
        out
    }

    /// A lone-surrogate payload must never be handed out as a `&str`; that is
    /// the `from_utf8_unchecked` precondition #10692 was about.
    #[test]
    fn a_lone_surrogate_payload_is_never_borrowed_as_str() {
        let bytes = wtf8(&[0x0065, 0xD800, 0x0301]);
        assert!(str::from_utf8(&bytes).is_err(), "fixture must be WTF-8");
        let subject = Wtf8Str::from_wtf8_bytes(&bytes);
        assert!(subject.as_str().is_none());
        // …but the code points are still reachable, surrogate included.
        assert_eq!(
            subject.code_points().collect::<Vec<_>>(),
            vec![0x65, 0xD800, 0x301]
        );
    }

    #[test]
    fn code_points_agree_with_chars_on_well_formed_text() {
        for s in ["", "abc", "äöü", "日本語", "😀🚀", "a\u{10FFFF}z"] {
            let expected: Vec<u32> = s.chars().map(u32::from).collect();
            assert_eq!(
                Wtf8Str::from_str(s).code_points().collect::<Vec<_>>(),
                expected,
                "{s:?}"
            );
            // The WTF-8 arm must decode identically to the `Chars` arm.
            assert_eq!(
                Wtf8Str::from_wtf8_bytes(s.as_bytes())
                    .code_points()
                    .collect::<Vec<_>>(),
                expected,
                "{s:?} (wtf8 arm)"
            );
        }
    }

    /// The oracle table, measured under Node v26.5.1 (`.node-version`) with
    /// `node --experimental-strip-types`. Perry matched it before #10692 only
    /// by the accident of `unicode-normalization` tolerating an out-of-range
    /// `char`; this pins it as a property of the implementation instead.
    #[cfg(feature = "string-normalize")]
    #[test]
    fn wtf8_normalize_matches_the_node_oracle() {
        use NormalizeForm::*;
        // (input units, NFC, NFD, NFKC, NFKD)
        let table: &[(&[u16], &[u16], &[u16], &[u16], &[u16])] = &[
            (&[0xD800], &[0xD800], &[0xD800], &[0xD800], &[0xD800]),
            (&[0xDC00], &[0xDC00], &[0xDC00], &[0xDC00], &[0xDC00]),
            (
                &[0x65, 0x301, 0xD800],
                &[0xE9, 0xD800],
                &[0x65, 0x301, 0xD800],
                &[0xE9, 0xD800],
                &[0x65, 0x301, 0xD800],
            ),
            (
                &[0xE9, 0xD800],
                &[0xE9, 0xD800],
                &[0x65, 0x301, 0xD800],
                &[0xE9, 0xD800],
                &[0x65, 0x301, 0xD800],
            ),
            (
                &[0xD800, 0xE9],
                &[0xD800, 0xE9],
                &[0xD800, 0x65, 0x301],
                &[0xD800, 0xE9],
                &[0xD800, 0x65, 0x301],
            ),
            // The semantically subtle one: the surrogate is a starter, so the
            // combining acute must NOT compose onto the `e` across it.
            (
                &[0x65, 0xD800, 0x301],
                &[0x65, 0xD800, 0x301],
                &[0x65, 0xD800, 0x301],
                &[0x65, 0xD800, 0x301],
                &[0x65, 0xD800, 0x301],
            ),
            (
                &[0xFB01, 0xD800],
                &[0xFB01, 0xD800],
                &[0xFB01, 0xD800],
                &[0x66, 0x69, 0xD800],
                &[0x66, 0x69, 0xD800],
            ),
            (
                &[0xDC00, 0xE9],
                &[0xDC00, 0xE9],
                &[0xDC00, 0x65, 0x301],
                &[0xDC00, 0xE9],
                &[0xDC00, 0x65, 0x301],
            ),
            (
                &[0x61, 0xD800, 0x62],
                &[0x61, 0xD800, 0x62],
                &[0x61, 0xD800, 0x62],
                &[0x61, 0xD800, 0x62],
                &[0x61, 0xD800, 0x62],
            ),
            (
                &[0xD800, 0x301],
                &[0xD800, 0x301],
                &[0xD800, 0x301],
                &[0xD800, 0x301],
                &[0xD800, 0x301],
            ),
            // U+212B ANGSTROM SIGN — a singleton decomposition, so NFC still
            // rewrites it even though the run has one scalar.
            (
                &[0x212B, 0xD800],
                &[0xC5, 0xD800],
                &[0x41, 0x30A, 0xD800],
                &[0xC5, 0xD800],
                &[0x41, 0x30A, 0xD800],
            ),
            (
                &[0xD800, 0xD800],
                &[0xD800, 0xD800],
                &[0xD800, 0xD800],
                &[0xD800, 0xD800],
                &[0xD800, 0xD800],
            ),
        ];
        for (input, nfc, nfd, nfkc, nfkd) in table {
            let bytes = wtf8(input);
            for (form, expected) in [(Nfc, nfc), (Nfd, nfd), (Nfkc, nfkc), (Nfkd, nfkd)] {
                let got = normalize(Wtf8Str::from_wtf8_bytes(&bytes), form);
                assert_eq!(
                    units(&got),
                    expected.to_vec(),
                    "{input:x?} {form:?} (Node v26.5.1 oracle)"
                );
            }
        }
    }

    /// The run-splitting path must not change well-formed text: for any input
    /// without surrogates it has to agree with `unicode-normalization` applied
    /// to the whole string at once.
    #[cfg(feature = "string-normalize")]
    #[test]
    fn run_splitting_agrees_with_whole_string_normalization() {
        use unicode_normalization::UnicodeNormalization;
        for s in [
            "",
            "abc",
            "e\u{301}",
            "\u{e9}",
            "\u{212b}",
            "\u{fb01}",
            "\u{1111}\u{1171}\u{11b6}",
            "a\u{323}\u{308}b",
            "日本語",
            "😀",
        ] {
            let subject = Wtf8Str::from_str(s);
            assert_eq!(normalize(subject, NormalizeForm::Nfc), {
                let v: String = s.nfc().collect();
                v.into_bytes()
            });
            // Force the run-splitting arm over the same (surrogate-free) text.
            let forced = transform_runs(s.as_bytes(), &mut |run, out| {
                let v: String = run.nfc().collect();
                out.extend_from_slice(v.as_bytes())
            });
            assert_eq!(forced, normalize(subject, NormalizeForm::Nfc), "{s:?}");
        }
    }

    #[test]
    fn to_lowercase_preserves_lone_surrogates() {
        let bytes = wtf8(&[0x41, 0xD800, 0x42]);
        assert_eq!(
            units(&to_lowercase(Wtf8Str::from_wtf8_bytes(&bytes))),
            vec![0x61, 0xD800, 0x62]
        );
        // A surrogate is neither Cased nor Case_Ignorable, so it breaks the
        // Final_Sigma context exactly the way run splitting does.
        let sigma = wtf8(&[0x3A3, 0xD800]);
        assert_eq!(
            units(&to_lowercase(Wtf8Str::from_wtf8_bytes(&sigma))),
            vec![0x3C3, 0xD800]
        );
    }

    #[test]
    fn to_lowercase_matches_str_to_lowercase_on_well_formed_text() {
        for s in ["", "ABC", "ÄÖÜ", "ΟΔΟΣ", "İ", "Σ", "aΣ"] {
            assert_eq!(
                to_lowercase(Wtf8Str::from_str(s)),
                s.to_lowercase().into_bytes(),
                "{s:?}"
            );
        }
    }

    /// A payload that is neither UTF-8 nor well-formed WTF-8 must still not
    /// reach `from_utf8_unchecked`; the run copies through verbatim.
    #[test]
    fn a_malformed_run_degrades_to_a_verbatim_copy() {
        // 0xC3 is a truncated 2-byte lead (#6085), then a lone surrogate.
        let mut bytes = vec![0xC3u8];
        bytes.extend_from_slice(&wtf8(&[0xD800]));
        let out = normalize(Wtf8Str::from_wtf8_bytes(&bytes), NormalizeForm::Nfc);
        assert_eq!(out, bytes);
    }
}
