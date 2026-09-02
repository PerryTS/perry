//! Node's `string_decoder` UTF-8 core: an INCREMENTAL decoder that replaces
//! invalid sequences with U+FFFD and, crucially, holds an incomplete
//! multi-byte sequence across a chunk boundary instead of mangling it.
//!
//! # Why this lives in perry-runtime (#9490)
//!
//! The logic below is not new — it was written for `node:string_decoder` and
//! has always lived in `perry-stdlib/src/string_decoder.rs`. But every stream
//! that honours `setEncoding("utf8")` lives one crate DOWN, in perry-runtime,
//! and perry-stdlib depends on perry-runtime rather than the other way round.
//! So the stream paths could not reach it, and each grew its own one-shot
//! decode instead:
//!
//!   * `process.stdin` passed the raw bytes to `js_string_from_bytes`, which
//!     does no validation at all — high bytes survived into the JS string and
//!     the WTF-8 length walk swallowed continuation bytes, so bytes 0..255
//!     came out as 158 code units with zero U+FFFD (Node: 256 and 128).
//!   * the generic `Readable` decoded each chunk independently, so a
//!     codepoint straddling two chunks became replacement characters.
//!
//! Moving the core down here — rather than copying it — keeps ONE
//! implementation: `perry-stdlib`'s `StringDecoderHandle` now embeds this
//! struct for its utf8 mode, so `node:string_decoder` and the stream decoders
//! cannot drift apart.

/// Incremental UTF-8 decode state: at most one partially-seen code point.
///
/// The fields are public because `node:string_decoder` exposes them verbatim
/// as its `lastNeed` / `lastTotal` / `lastChar` properties.
#[derive(Debug, Clone, Default)]
pub struct Utf8StreamDecoder {
    /// Number of bytes still needed to complete the current code point
    /// (0 when no partial point is buffered).
    pub last_need: u8,
    /// Total byte length of the in-progress code point (2, 3, or 4).
    pub last_total: u8,
    /// Up to 4 bytes of partial code point captured from prior writes.
    pub last_char: [u8; 4],
    /// How many bytes of `last_char` are valid; never larger than 4.
    pub last_char_len: u8,
}

impl Utf8StreamDecoder {
    /// `const` so a decoder can live in a `static Mutex<_>` — `process.stdin`
    /// is a process-global stream and needs exactly one decode state.
    pub const fn new() -> Self {
        Utf8StreamDecoder {
            last_need: 0,
            last_total: 0,
            last_char: [0; 4],
            last_char_len: 0,
        }
    }

    /// The incomplete sequence currently being held, if any. Callers that
    /// cannot keep a decoder alive between chunks (the generic `Readable`
    /// stores its state in a per-stream hidden field) round-trip these bytes
    /// and re-prefix them onto the next chunk, which is equivalent.
    pub fn pending_bytes(&self) -> &[u8] {
        &self.last_char[..self.last_char_len as usize]
    }

    /// True while an incomplete multi-byte sequence is being held.
    pub fn has_pending(&self) -> bool {
        self.last_need > 0
    }

    /// Forget any held partial without emitting a replacement. Used when a
    /// stream is destroyed or re-opened rather than ended.
    pub fn reset(&mut self) {
        self.last_need = 0;
        self.last_total = 0;
        self.last_char_len = 0;
        self.last_char = [0; 4];
    }

    /// Decode `bytes`, holding back any trailing incomplete sequence.
    /// Returns "" when the whole input was consumed into the held partial —
    /// callers must NOT emit a `'data'` event for an empty result, matching
    /// Node.
    pub fn write(&mut self, bytes: &[u8]) -> String {
        write_utf8(self, bytes)
    }

    /// Flush at end-of-stream: any held partial becomes a single U+FFFD,
    /// exactly as `StringDecoder.prototype.end` does. Node emits this as its
    /// own final `'data'` event, before `'end'`.
    pub fn end(&mut self, bytes: Option<&[u8]>) -> String {
        end_utf8(self, bytes)
    }
}

/// Detect a multi-byte UTF-8 lead in the final 0–3 bytes of `buf`.
/// Returns the number of bytes that should be buffered for the next
/// write (so they aren't returned as garbled output). Mirrors the
/// `utf8CheckIncomplete` function in Node's `lib/string_decoder.js`.
fn utf8_check_incomplete(state: &mut Utf8StreamDecoder, buf: &[u8]) -> usize {
    let mut i = buf.len();
    // Walk back from the end of the buffer up to 3 bytes — the longest
    // UTF-8 lead sequence the trailing bytes could need to wait for.
    let walk = if buf.len() >= 3 { 3 } else { buf.len() };
    let mut steps = 0usize;
    while steps < walk {
        i -= 1;
        steps += 1;
        let b = buf[i];
        // Continuation byte 10xxxxxx — keep walking.
        if (b & 0xC0) == 0x80 {
            continue;
        }
        // 4-byte lead 11110xxx.
        if (b & 0xF8) == 0xF0 {
            // We've already walked `steps - 1` continuation bytes plus
            // this lead; we need 4 total, so we still need
            // `4 - steps` bytes.
            if steps < 4 {
                state.last_need = (4 - steps) as u8;
                state.last_total = 4;
                let start = buf.len() - steps;
                state.last_char_len = steps as u8;
                state.last_char[..steps].copy_from_slice(&buf[start..]);
                return steps;
            }
            return 0;
        }
        // 3-byte lead 1110xxxx.
        if (b & 0xF0) == 0xE0 {
            if steps < 3 {
                state.last_need = (3 - steps) as u8;
                state.last_total = 3;
                let start = buf.len() - steps;
                state.last_char_len = steps as u8;
                state.last_char[..steps].copy_from_slice(&buf[start..]);
                return steps;
            }
            return 0;
        }
        // 2-byte lead 110xxxxx.
        if (b & 0xE0) == 0xC0 {
            if steps < 2 {
                state.last_need = (2 - steps) as u8;
                state.last_total = 2;
                let start = buf.len() - steps;
                state.last_char_len = steps as u8;
                state.last_char[..steps].copy_from_slice(&buf[start..]);
                return steps;
            }
            return 0;
        }
        // ASCII byte 0xxxxxxx — nothing to buffer.
        return 0;
    }
    0
}

/// Decode `bytes` against the existing partial-codepoint state, mutating
/// `state` to reflect any new trailing partial. Returns the decoded
/// string. UTF-8 invalid sequences are replaced with U+FFFD, matching
/// Node's `lossy` UTF-8 decoder behavior.
fn write_utf8(state: &mut Utf8StreamDecoder, bytes: &[u8]) -> String {
    let mut out = String::new();

    // Stitch the buffered partial together with the new input first.
    if state.last_need > 0 {
        let need = state.last_need as usize;
        if bytes.len() < need {
            // Still incomplete — append what we can and exit empty.
            let new_len = state.last_char_len as usize + bytes.len();
            if new_len <= 4 {
                state.last_char[state.last_char_len as usize..new_len].copy_from_slice(bytes);
                state.last_char_len = new_len as u8;
                state.last_need -= bytes.len() as u8;
            } else {
                // Defensive: should never happen given UTF-8 is at most 4
                // bytes, but if upstream feeds garbage we reset rather
                // than overrun.
                state.last_need = 0;
                state.last_total = 0;
                state.last_char_len = 0;
            }
            return out;
        }

        // We have enough new bytes to complete the buffered point.
        let total = state.last_total as usize;
        let buffered = state.last_char_len as usize;
        let take_new = total - buffered;
        let mut cp = Vec::with_capacity(total);
        cp.extend_from_slice(&state.last_char[..buffered]);
        cp.extend_from_slice(&bytes[..take_new]);

        match std::str::from_utf8(&cp) {
            Ok(s) => out.push_str(s),
            Err(_) => out.push('\u{FFFD}'),
        }
        state.last_need = 0;
        state.last_total = 0;
        state.last_char_len = 0;

        // The "rest" continues below — chop off the consumed prefix.
        let rest = &bytes[take_new..];
        // Recurse on the tail so trailing partials get caught.
        out.push_str(&write_utf8_tail(state, rest));
        return out;
    }

    out.push_str(&write_utf8_tail(state, bytes));
    out
}

/// Tail half of `write_utf8`: assumes `state.last_need == 0` on entry.
/// Splits a trailing incomplete code point off into `state`.
fn write_utf8_tail(state: &mut Utf8StreamDecoder, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let trail = utf8_check_incomplete(state, bytes);
    let head = &bytes[..bytes.len() - trail];
    String::from_utf8_lossy(head).into_owned()
}

/// `decoder.end([buf?])` — flush any incomplete state as U+FFFD, matching
/// Node's behavior.
fn end_utf8(state: &mut Utf8StreamDecoder, bytes: Option<&[u8]>) -> String {
    let mut out = match bytes {
        Some(b) => write_utf8(state, b),
        None => String::new(),
    };
    if state.last_need > 0 {
        out.push('\u{FFFD}');
        state.last_need = 0;
        state.last_total = 0;
        state.last_char_len = 0;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_euro_sign() {
        // U+20AC EURO SIGN = E2 82 AC in UTF-8.
        let mut d = Utf8StreamDecoder::new();
        assert_eq!(d.write(&[0xE2, 0x82]), "");
        assert_eq!(d.last_need, 1);
        assert_eq!(d.last_total, 3);
        assert!(d.has_pending());
        assert_eq!(d.write(&[0xAC]), "\u{20AC}");
        assert_eq!(d.last_need, 0);
        assert!(!d.has_pending());
    }

    #[test]
    fn emoji_split_at_every_boundary() {
        // U+1F600 = F0 9F 98 80. #9490's fixture splits it 1/3, 2/2 and 3/1;
        // all three must reassemble to the same single code point.
        let emoji = [0xF0u8, 0x9F, 0x98, 0x80];
        for cut in 1..4usize {
            let mut d = Utf8StreamDecoder::new();
            let a = d.write(&emoji[..cut]);
            let b = d.write(&emoji[cut..]);
            assert_eq!(a, "", "cut {cut} must emit nothing yet");
            assert_eq!(b, "\u{1F600}", "cut {cut}");
            assert!(!d.has_pending());
        }
    }

    #[test]
    fn end_flushes_partial_as_one_replacement() {
        let mut d = Utf8StreamDecoder::new();
        assert_eq!(d.write(&[0x41, 0xE2, 0x82]), "A");
        assert_eq!(d.end(None), "\u{FFFD}");
        assert!(!d.has_pending());
    }

    #[test]
    fn all_256_bytes_match_node() {
        // Node: 256 code units, 128 of them U+FFFD.
        let bytes: Vec<u8> = (0..=255u8).collect();
        let mut d = Utf8StreamDecoder::new();
        let mut out = d.write(&bytes);
        out.push_str(&d.end(None));
        assert_eq!(out.encode_utf16().count(), 256);
        assert_eq!(out.chars().filter(|c| *c == '\u{FFFD}').count(), 128);
    }

    #[test]
    fn invalid_sequences_replace_per_whatwg() {
        let mut d = Utf8StreamDecoder::new();
        // lone continuation, 2-byte overlong, 3-byte overlong, surrogate.
        assert_eq!(d.write(&[0x80]), "\u{FFFD}");
        assert_eq!(d.write(&[0xC0, 0x80]), "\u{FFFD}\u{FFFD}");
        assert_eq!(d.write(&[0xE0, 0x80, 0xAF]), "\u{FFFD}\u{FFFD}\u{FFFD}");
        assert_eq!(d.write(&[0xED, 0xA0, 0x80]), "\u{FFFD}\u{FFFD}\u{FFFD}");
    }

    #[test]
    fn reset_drops_the_partial_without_emitting() {
        let mut d = Utf8StreamDecoder::new();
        assert_eq!(d.write(&[0xF0, 0x9F]), "");
        d.reset();
        assert!(!d.has_pending());
        assert_eq!(d.end(None), "");
    }

    #[test]
    fn ascii_round_trips() {
        let mut d = Utf8StreamDecoder::new();
        assert_eq!(d.write(b"hello"), "hello");
        assert_eq!(d.last_need, 0);
    }
}
