//! Source-form admission for lazy stringify. All scratch is native and lives
//! only for this call; no managed values are constructed during the proof.

use crate::json_tape::*;

struct ObjectKeys {
    first: usize,
    end: usize,
    last_index: Option<u32>,
    saw_name: bool,
}

/// Prove that copying these tokens preserves compact JSON string spelling and
/// property enumeration. Number spelling is checked by the caller separately.
/// Escaped keys and objects wider than 32 keys conservatively materialize;
/// the bound prevents duplicate detection from becoming quadratic on wide input.
#[cfg(test)]
fn source_is_copyable(tape: &[TapeEntry], blob: &[u8], root: usize) -> bool {
    visit_copyable_numbers(tape, blob, root, |_, _, _| Some(())).is_some()
}

/// Visit numbers during the source proof, so normalization does not need a
/// second tape walk or a second scan of each number token. The callback may
/// grow native buffers but must not allocate managed objects or collect.
pub(super) fn visit_copyable_numbers(
    tape: &[TapeEntry],
    blob: &[u8],
    root: usize,
    mut number: impl FnMut(usize, usize, &[u8]) -> Option<()>,
) -> Option<()> {
    // Reject raw WTF-8 surrogates once for the entire source. Validating every
    // short key and string separately pays the validator's setup per token.
    let text = simdutf8::basic::from_utf8(blob).ok()?;
    let root_entry = tape.get(root)?;
    if root_entry.kind != KIND_ARR_START {
        return None;
    }
    let end = root_entry.link as usize;
    let entries = tape.get(root..=end)?;
    let mut cursor = root_entry.offset as usize;
    let mut previous = KIND_ARR_START;
    let mut objects: Vec<ObjectKeys> = Vec::new();
    let mut keys: Vec<&[u8]> = Vec::new();
    for (relative, entry) in entries.iter().enumerate() {
        let at = entry.offset as usize;
        // Tape omits commas and colons. Require the exact compact separator,
        // including adjacent empty/nested containers, rather than skipping ws.
        let separator = if previous == KIND_KEY {
            Some(b':')
        } else if matches!(previous, KIND_ARR_START | KIND_OBJ_START)
            || matches!(entry.kind, KIND_ARR_END | KIND_OBJ_END)
        {
            None
        } else {
            Some(b',')
        };
        if at.checked_sub(cursor)? != usize::from(separator.is_some())
            || separator.is_some_and(|byte| blob.get(cursor) != Some(&byte))
        {
            return None;
        }
        cursor = match entry.kind {
            KIND_OBJ_START => {
                objects.push(ObjectKeys {
                    first: keys.len(),
                    end: entry.link as usize,
                    last_index: None,
                    saw_name: false,
                });
                at + 1
            }
            KIND_OBJ_END => {
                let frame = objects.pop()?;
                if frame.end != root + relative {
                    return None;
                }
                keys.truncate(frame.first);
                at + 1
            }
            KIND_ARR_START | KIND_ARR_END => at + 1,
            KIND_STRING | KIND_KEY => {
                let next = entries.get(relative + 1)?;
                let end = canonical_string_end(blob, entry, next)?;
                if entry.kind == KIND_KEY {
                    let key_text = text.get(at + 1..end - 1)?;
                    let key = key_text.as_bytes();
                    let frame = objects.last_mut()?;
                    let siblings = &keys[frame.first..];
                    if siblings.len() >= 32 || siblings.contains(&key) {
                        return None;
                    }
                    // Ordinary names cannot be array indices. Avoid calling
                    // the general index parser for each key in a record.
                    let index = if key.first().is_some_and(u8::is_ascii_digit) {
                        crate::object::canonical_array_index(key_text)
                    } else {
                        None
                    };
                    if let Some(index) = index {
                        if frame.saw_name || frame.last_index.is_some_and(|last| index <= last) {
                            return None;
                        }
                        frame.last_index = Some(index);
                    } else {
                        frame.saw_name = true;
                    }
                    keys.push(key);
                }
                end
            }
            KIND_NUMBER => {
                let (end, token) = super::stringify_api::json_number_token(blob, at)?;
                number(at, end, token)?;
                end
            }
            KIND_TRUE | KIND_NULL => at + 4,
            KIND_FALSE => at + 5,
            _ => return None,
        };
        previous = entry.kind;
    }
    if previous != KIND_ARR_END || !objects.is_empty() || cursor > blob.len() {
        return None;
    }
    Some(())
}

fn canonical_string_end(blob: &[u8], entry: &TapeEntry, next: &TapeEntry) -> Option<usize> {
    // A valid tape identifies the next token. Its required compact separator
    // fixes this string's closing quote without another quote search. The
    // adjacent-token check in the caller then validates that separator byte.
    let gap =
        usize::from(entry.kind == KIND_KEY || !matches!(next.kind, KIND_ARR_END | KIND_OBJ_END));
    let start = entry.offset as usize;
    let end = (next.offset as usize).checked_sub(gap)?;
    if end.checked_sub(start)? < 2
        || blob.get(start) != Some(&b'"')
        || blob.get(end - 1) != Some(&b'"')
    {
        return None;
    }
    let mut body = blob.get(start + 1..end - 1)?;
    while let Some(at) = super::simd::find_quote_or_backslash(body) {
        if entry.kind == KIND_KEY || body[at] != b'\\' {
            return None;
        }
        match body.get(at + 1)? {
            b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't' => body = body.get(at + 2..)?,
            // Slash and Unicode escapes need normalization. Canonical Unicode
            // control/surrogate escapes conservatively use materialization too.
            _ => return None,
        }
    }
    Some(end)
}

#[cfg(test)]
#[path = "stringify_lazy_tests.rs"]
mod tests;
