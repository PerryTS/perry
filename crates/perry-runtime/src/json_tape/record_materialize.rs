//! Materialize one small lazy record using the ordinary JSON construction batch.
//! The tape supplies its exact byte range; larger records keep the rooted tape
//! walker. No source-identity cache is used for a slice of the enclosing blob.

use super::*;

const MAX_RECORD_BYTES: usize = 512;

pub(super) unsafe fn try_small_record(
    source: &TapeSource<'_, '_>,
    scope: &crate::gc::RuntimeHandleScope,
    start_idx: usize,
) -> Option<JSValue> {
    let entry = source.entry(start_idx)?;
    if entry.kind != KIND_OBJ_START {
        return None;
    }
    let end = source.entry(entry.link as usize)?;
    if end.kind != KIND_OBJ_END {
        return None;
    }
    let len = (end.offset as usize)
        .checked_sub(entry.offset as usize)?
        .checked_add(1)?;
    if len > MAX_RECORD_BYTES {
        return None;
    }

    // Reuse the bounded parse boundary policy. An unconditional trigger here
    // also collects for unrelated old/tape pressure on every sparse read,
    // changing the scheduling of an otherwise cheap record construction.
    // Full-GC mode and active incremental cycles retain ordinary servicing.
    // TapeSource re-reads the rooted blob after a possible move.
    if !crate::gc::gen_gc_enabled() || crate::gc::gc_budgeted_cycle_active() {
        crate::gc::gc_check_trigger();
    } else {
        crate::gc::gc_collect_pending_suppressed_parse();
    }
    let value_handle = {
        let _suppress = crate::gc::GcSuppressScope::new();
        let bytes = source.bytes_from_offset(entry.offset as usize).get(..len)?;
        let mut parser = crate::json::DirectParser::new_batched(bytes);
        let value = parser.parse_value();
        if !parser.finish() {
            return None;
        }
        scope.root_nanbox_u64(value.bits())
    };
    // At most 63 more records of <=512 source bytes can precede the next
    // pressure observation, using the existing tiny-parse completion counter.
    // This schedules work; the completed record is already a rewritable root.
    crate::gc::gc_schedule_tiny_parse_boundary_collection_if_pressure();
    json_tape_safepoint(
        JsonTapeSafepoint::SmallRecordBatchRooted,
        JSValue::from_bits(value_handle.get_nanbox_u64()).as_pointer::<u8>() as usize,
    );
    Some(JSValue::from_bits(value_handle.get_nanbox_u64()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_lazy_record_batch_preserves_duplicate_escape_and_nested_values() {
        // Expected output checked against the repository's pinned Node oracle.
        // The legacy tape walker drops lone surrogates, so it cannot serve as
        // the oracle for the escaped-string case.
        for (text, expected) in [
            (
                r#"{"id":42,"name":"user_42","tags":["red","blue"],"active":true}"#,
                r#"{"id":42,"name":"user_42","tags":["red","blue"],"active":true}"#,
            ),
            (
                r#"{"a":1,"\u0061":2,"nested":{"x":1,"x":3},"s":"\ud800\n"}"#,
                r#"{"a":2,"nested":{"x":3},"s":"\ud800\n"}"#,
            ),
            (
                r#"{"0":0,"__proto__":{"polluted":true},"v":-0,"n":1e300}"#,
                r#"{"0":0,"__proto__":{"polluted":true},"v":0,"n":1e+300}"#,
            ),
        ] {
            unsafe {
                let tape = build_tape(text.as_bytes()).unwrap();
                let source = TapeSource::Borrowed {
                    tape: &tape.entries,
                    bytes: text.as_bytes(),
                };
                let scope = crate::gc::RuntimeHandleScope::new();
                let value = try_small_record(&source, &scope, 0)
                    .expect("bounded record must reach the batch producer");
                let batch = crate::json::js_json_stringify(f64::from_bits(value.bits()), 0);
                assert_eq!(crate::string::string_as_str(batch), expected);
            }
        }
    }

    #[test]
    fn small_lazy_record_batch_uses_the_element_range() {
        let text = br#"[{"id":1},{"id":2,"s":"right"},{"id":3}]"#;
        let tape = build_tape(text).unwrap();
        unsafe {
            let first_end = tape.entries[1].link as usize;
            let source = TapeSource::Borrowed {
                tape: &tape.entries,
                bytes: text,
            };
            let scope = crate::gc::RuntimeHandleScope::new();
            let value = try_small_record(&source, &scope, first_end + 1).unwrap();
            let output = crate::json::js_json_stringify(f64::from_bits(value.bits()), 0);
            assert_eq!(
                crate::string::string_as_str(output),
                r#"{"id":2,"s":"right"}"#
            );
        }
    }

    #[test]
    fn small_lazy_record_batch_declines_large_records_and_nonobjects() {
        for text in [
            format!(r#"{{"s":"{}"}}"#, "x".repeat(MAX_RECORD_BYTES)),
            "[1,2]".to_owned(),
            "null".to_owned(),
        ] {
            let tape = build_tape(text.as_bytes()).unwrap();
            unsafe {
                let scope = crate::gc::RuntimeHandleScope::new();
                let source = TapeSource::Borrowed {
                    tape: &tape.entries,
                    bytes: text.as_bytes(),
                };
                assert!(try_small_record(&source, &scope, 0).is_none());
            }
        }
    }
}
