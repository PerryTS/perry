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
pub(super) fn source_is_copyable(tape: &[TapeEntry], blob: &[u8], root: usize) -> bool {
    copyable(tape, blob, root).is_some()
}

fn copyable(tape: &[TapeEntry], blob: &[u8], root: usize) -> Option<()> {
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
        let separator: &[u8] = if previous == KIND_KEY {
            b":"
        } else if matches!(previous, KIND_ARR_START | KIND_OBJ_START)
            || matches!(entry.kind, KIND_ARR_END | KIND_OBJ_END)
        {
            b""
        } else {
            b","
        };
        if blob.get(cursor..at)? != separator {
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
                let end = canonical_string_end(blob, at, entry.kind == KIND_KEY)?;
                if entry.kind == KIND_KEY {
                    let key = blob.get(at + 1..end - 1)?;
                    let frame = objects.last_mut()?;
                    let siblings = &keys[frame.first..];
                    if siblings.len() >= 32 || siblings.contains(&key) {
                        return None;
                    }
                    let text = std::str::from_utf8(key).ok()?;
                    if let Some(index) = crate::object::canonical_array_index(text) {
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
            KIND_NUMBER => super::stringify_api::json_number_token(blob, at)?.0,
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

fn canonical_string_end(blob: &[u8], start: usize, key: bool) -> Option<usize> {
    if blob.get(start) != Some(&b'"') {
        return None;
    }
    let mut at = start + 1;
    loop {
        at += super::simd::find_string_terminator(blob.get(at..)?)?;
        match *blob.get(at)? {
            b'"' => {
                // Raw WTF-8 surrogate bytes must be escaped by stringify.
                // SIMD validation accepts ordinary Unicode without decoding it.
                simdutf8::basic::from_utf8(blob.get(start + 1..at)?).ok()?;
                return Some(at + 1);
            }
            b'\\' if !key => {
                match blob.get(at + 1)? {
                    b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't' => at += 2,
                    // Slash escapes and Unicode escapes need normalization.
                    // Materialize even already-canonical \u control/surrogate
                    // escapes instead of guessing about decoded code units.
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
#[path = "stringify_lazy_tests.rs"]
mod tests;
