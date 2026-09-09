//! Derive a dominant unescaped token's length from its already-counted owner.
//! Only bounded ASCII surroundings are scanned; no input/output view or cache
//! is retained, and the ordinary constructor still owns allocation and copying.

use super::DirectParser;

const MAX_SURROUNDING_BYTES: usize = 512;

impl DirectParser<'_> {
    // Keep the proof and the counted constructor out of the ordinary string
    // value path: small records should not carry their extra allocation state.
    #[inline(never)]
    pub(super) unsafe fn alloc_large_borrowed_string(
        &mut self,
        bytes: &[u8],
    ) -> *mut crate::string::StringHeader {
        match self.borrowed_source_utf16_len(bytes) {
            Some(units) => {
                crate::string::string_from_json_bytes_counted(&mut self.batch, bytes, units)
            }
            None => crate::string::string_from_json_bytes(&mut self.batch, bytes),
        }
    }

    /// Only reached for a large borrowed token after its closing quote was
    /// consumed. The parse API roots `source` and suppresses collection through
    /// construction. Standalone byte parsers and source slices must decline.
    #[inline(never)]
    pub(super) unsafe fn borrowed_source_utf16_len(&self, bytes: &[u8]) -> Option<u32> {
        if self.source.is_null()
            || (*self.source).byte_len as usize != self.input.len()
            || crate::string::string_data(self.source) != self.input.as_ptr()
        {
            return None;
        }
        let end = self.pos.checked_sub(1)?;
        let start = end.checked_sub(bytes.len())?;
        if self.input.get(start..end)?.as_ptr() != bytes.as_ptr() {
            return None;
        }
        count_from_ascii_surroundings(self.input, start, end, (*self.source).utf16_len)
    }
}

fn count_from_ascii_surroundings(
    input: &[u8],
    start: usize,
    end: usize,
    source_units: u32,
) -> Option<u32> {
    let bytes = input.get(start..end)?;
    let outside = input.len().checked_sub(bytes.len())?;
    if outside > MAX_SURROUNDING_BYTES
        || !input[..start].is_ascii()
        || !input[end..].is_ascii()
        // The bounded WTF-8 counter can step across a truncated token's end
        // into its closing quote. Subtraction would then disagree with counting
        // the token alone. The same conservative three-byte proof is used by
        // stringify when adding quotes; it is not a UTF-8 validity assertion.
        || super::super::stringify_string::has_incomplete_tail(bytes)
    {
        return None;
    }
    let units = source_units.checked_sub(outside as u32)?;
    (units as usize <= bytes.len()).then_some(units)
}

#[cfg(test)]
#[path = "parser_source_string_tests.rs"]
mod tests;
