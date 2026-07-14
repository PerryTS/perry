//! UTF-16 <-> byte index conversion.
//!
//! JS string indices are UTF-16 code units, and `StringHeader::utf16_len`
//! (`str.length`) reports them — but Rust byte offsets are what the match
//! engines hand back. Converting with `chars().count()` yields *Unicode
//! scalars*, which disagrees with `str.length` / `charAt` on the same string
//! at and past any astral character.
//!
//! These live outside `exec_array` deliberately: that module is behind
//! `#[cfg(feature = "regex-engine")]`, but `replace_fn` and `match_string`
//! are not, and they need these too. Keeping the helpers here means a
//! `regex-engine`-off build still compiles. (Same class of cross-gate
//! dependency as #6303.)

/// UTF-16 code-unit index -> byte offset.
pub(super) fn utf16_index_to_byte(s: &str, utf16_index: usize) -> usize {
    if utf16_index == 0 {
        return 0;
    }
    let mut units = 0usize;
    for (byte, ch) in s.char_indices() {
        if units >= utf16_index {
            return byte;
        }
        units += ch.len_utf16();
    }
    s.len()
}

/// Byte offset -> UTF-16 code-unit index.
pub(super) fn byte_index_to_utf16_index(s: &str, byte_index: usize) -> usize {
    s[..byte_index.min(s.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}
