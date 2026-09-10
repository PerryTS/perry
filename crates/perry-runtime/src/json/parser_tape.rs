//! Construct shallow records directly from an already validated lazy tape.
//! Scalar decoders and final object/array constructors are the direct parser's
//! existing ones. No leaf-end metadata, new cache, or collector policy is added.

use super::*;
use crate::json::construction_array::ConstructionArray;
use crate::json_tape::{
    TapeEntry, KIND_ARR_END, KIND_ARR_START, KIND_FALSE, KIND_KEY, KIND_NULL, KIND_NUMBER,
    KIND_OBJ_END, KIND_OBJ_START, KIND_STRING, KIND_TRUE,
};

fn scalar_kind(kind: u8) -> bool {
    matches!(
        kind,
        KIND_STRING | KIND_NUMBER | KIND_TRUE | KIND_FALSE | KIND_NULL
    )
}

fn subtree_end(tape: &[TapeEntry], index: usize) -> Option<usize> {
    let entry = tape.get(index)?;
    match entry.kind {
        KIND_OBJ_START | KIND_ARR_START => {
            let end = entry.link as usize;
            let expected = if entry.kind == KIND_OBJ_START {
                KIND_OBJ_END
            } else {
                KIND_ARR_END
            };
            (end > index && tape.get(end)?.kind == expected).then_some(end)
        }
        kind if scalar_kind(kind) => Some(index),
        _ => None,
    }
}

// Decline unsupported records before allocating any of their fields. Keep wide
// objects, nested objects and nested arrays on the existing direct producer.
fn shallow_record_fields(tape: &[TapeEntry], start: usize) -> Option<usize> {
    if tape.get(start)?.kind != KIND_OBJ_START {
        return None;
    }
    let end = subtree_end(tape, start)?;
    let mut index = start + 1;
    let mut fields = 0;
    while index < end {
        if fields == 8 || tape.get(index)?.kind != KIND_KEY {
            return None;
        }
        let value = index + 1;
        let entry = tape.get(value)?;
        let value_end = subtree_end(tape, value)?;
        if entry.kind == KIND_ARR_START {
            if !tape
                .get(value + 1..value_end)?
                .iter()
                .all(|entry| scalar_kind(entry.kind))
            {
                return None;
            }
        } else if !scalar_kind(entry.kind) {
            return None;
        }
        if value_end >= end {
            return None;
        }
        index = value_end + 1;
        fields += 1;
    }
    (index == end).then_some(fields)
}

impl DirectParser<'_> {
    /// Only call with a validated tape for this parser's bytes, with the input
    /// and owning lazy header rooted and collection suppressed for this whole
    /// call. No user callback runs here. Every returned payload is complete;
    /// the caller roots it before lifting suppression and patches cached slots
    /// using the existing identity-preserving publication path.
    ///
    /// A decline leaves the parser at its original position, before allocation.
    #[inline(never)]
    pub(crate) unsafe fn materialize_tape_records(
        &mut self,
        tape: &[TapeEntry],
        length: u32,
    ) -> Option<JSValue> {
        if tape.first()?.kind != KIND_ARR_START || length == 0 {
            return None;
        }
        let end = subtree_end(tape, 0)?;
        // Admission is deliberately local to full materialization. Other root
        // kinds, sparse reads and all parse-time routing keep their old path.
        shallow_record_fields(tape, 1)?;
        let mut array = ConstructionArray::new(&mut self.batch, length);
        let mut index = 1;
        while index < end {
            let value_end = subtree_end(tape, index).expect("validated value subtree");
            let value = if let Some(fields) = shallow_record_fields(tape, index) {
                self.materialize_tape_record(tape, index, fields)
            } else {
                // The parser retains its shape hint across both producers.
                // Unsupported subtrees are parsed once at their source offset.
                self.pos = tape[index].offset as usize;
                self.parse_value()
            };
            array.push(&mut self.batch, value);
            index = value_end + 1;
        }
        self.pos = tape[end].offset as usize + 1;
        Some(JSValue::object_ptr(array.finish(&self.batch).cast()))
    }

    unsafe fn materialize_tape_scalar(&mut self, entry: &TapeEntry) -> JSValue {
        self.pos = entry.offset as usize;
        match entry.kind {
            KIND_STRING => self.parse_string_value(),
            KIND_NUMBER => self.parse_number(),
            KIND_TRUE => JSValue::bool(true),
            KIND_FALSE => JSValue::bool(false),
            KIND_NULL => JSValue::null(),
            _ => unreachable!("shallow-record admission checked scalar kinds"),
        }
    }

    unsafe fn materialize_tape_field(&mut self, tape: &[TapeEntry], index: usize) -> JSValue {
        let entry = &tape[index];
        if entry.kind != KIND_ARR_START {
            return self.materialize_tape_scalar(entry);
        }
        let end = entry.link as usize;
        let mut array = ConstructionArray::new(&mut self.batch, (end - index - 1) as u32);
        for entry in &tape[index + 1..end] {
            let value = self.materialize_tape_scalar(entry);
            array.push(&mut self.batch, value);
        }
        JSValue::object_ptr(array.finish(&self.batch).cast())
    }

    unsafe fn materialize_tape_record(
        &mut self,
        tape: &[TapeEntry],
        start: usize,
        field_count: usize,
    ) -> JSValue {
        let warm = (self.hot_shape_len != 0 && !self.hot_shape_array.is_null()).then_some((
            self.hot_shape_len,
            self.hot_shape_keys,
            self.hot_shape_array,
            self.hot_shape_id,
        ));
        let mut warm_matches = warm.is_some();
        let mut warm_slot = 0;
        let mut keys = [std::ptr::null(); 8];
        let mut values = [JSValue::undefined(); 8];
        let mut used = 0;
        let mut index = start + 1;
        for _ in 0..field_count {
            self.pos = tape[index].offset as usize;
            let expected = warm.as_ref().and_then(|(len, keys, _, _)| {
                (warm_matches && warm_slot < *len).then(|| keys[warm_slot])
            });
            let (key, matched_spelling) = match expected {
                Some(expected) => self.parse_string_bytes_expected(expected),
                None => self.parse_string_bytes().map(|key| (key, false)),
            }
            .expect("validated JSON key");
            let value_index = index + 1;
            let value = self.materialize_tape_field(tape, value_index);
            let key_bytes = key.as_bytes();
            let key_ptr = if let Some(expected) =
                expected.filter(|&ptr| matched_spelling || json_key_bytes_equal(ptr, key_bytes))
            {
                warm_slot += 1;
                expected
            } else {
                warm_matches = false;
                cached_parse_key_ptr(key_bytes)
            };
            // A warm prefix can refer to the same spelling under an older
            // cache identity. Preserve the direct parser's duplicate-key rule.
            let compare_spelling = warm_slot != 0 && !warm_matches;
            if let Some(slot) = keys[..used].iter().position(|&ptr| {
                ptr == key_ptr || (compare_spelling && json_key_bytes_equal(ptr, key_bytes))
            }) {
                values[slot] = value;
            } else {
                keys[used] = key_ptr;
                values[used] = value;
                used += 1;
            }
            index = subtree_end(tape, value_index).expect("admitted field") + 1;
        }
        let (keys_array, shape_id) = match warm {
            Some((len, _, array, id)) if warm_matches && warm_slot == len && used == len => {
                (array, id)
            }
            _ => self.parse_shape_keys_array_hot(&keys[..used]),
        };
        JSValue::object_ptr(
            crate::object::object_from_json_fields_preinstalled(
                &mut self.batch,
                keys_array,
                shape_id,
                &values[..used],
            )
            .cast(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_tape_batch_records_match_direct_decoding_and_fallbacks() {
        let text = r#"[{"id":1,"s":"warm","tags":["red",true,null,2]},{"id":2,"s":"\u0061\ud800\n","tags":[]},{"id":3,"id":4,"__proto__":5},{"id":6,"nested":{"x":1,"x":2}},{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8,"i":9},{"n":-0,"f":0.10000000000000001,"big":1e300},null,[1,2],{}, {"s":"é😀"}]"#;
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let _suppress = crate::gc::GcSuppressScope::new();
            let tape = crate::json_tape::build_tape(text.as_bytes()).unwrap();
            let mut parser = DirectParser::new_batched(text.as_bytes());
            let value = parser
                .materialize_tape_records(&tape.entries, 10)
                .expect("producer must run");
            assert!(parser.finish());
            let root = scope.root_nanbox_u64(value.bits());
            let mut direct = DirectParser::new_batched(text.as_bytes());
            let expected = direct.parse_value();
            assert!(direct.finish());
            let expected = scope.root_nanbox_u64(expected.bits());
            let output = crate::json::js_json_stringify(f64::from_bits(root.get_nanbox_u64()), 0);
            let output = crate::string::string_as_str(output).to_owned();
            let reference =
                crate::json::js_json_stringify(f64::from_bits(expected.get_nanbox_u64()), 0);
            assert_eq!(output, crate::string::string_as_str(reference));
        }
    }

    #[test]
    fn json_tape_batch_decline_preserves_direct_parser_position() {
        for text in ["[]", "[1,2]", r#"[{"nested":{"x":1}}]"#, r#"{"a":1}"#] {
            unsafe {
                let _suppress = crate::gc::GcSuppressScope::new();
                let tape = crate::json_tape::build_tape(text.as_bytes()).unwrap();
                let mut parser = DirectParser::new_batched(text.as_bytes());
                assert!(parser.materialize_tape_records(&tape.entries, 1).is_none());
                assert_eq!(parser.pos, 0);
                parser.parse_value();
                assert!(parser.finish());
            }
        }
    }
}
