//! Complete a lazy array without rebuilding its already-cached subtrees.
//! The validated tape supplies offsets; ordinary parsing builds missing values.

use super::*;
use crate::json::construction_array::ConstructionArray;
use crate::json_tape::{TapeEntry, KIND_ARR_END, KIND_ARR_START, KIND_OBJ_START};

impl DirectParser<'_> {
    /// The tape must describe these input bytes and exactly `cached.len()` root
    /// elements. Every set bitmap bit must name a live cached value. The input,
    /// tape and cache owner stay rooted, with collection suppressed and no user
    /// callbacks throughout construction. The caller roots the finished array
    /// before lifting suppression. Declines happen before allocation/consumption.
    #[inline(never)]
    pub(crate) unsafe fn materialize_cached_array(
        &mut self,
        tape: &[TapeEntry],
        cached: &[JSValue],
        bitmap: &[u64],
    ) -> Option<JSValue> {
        let root = tape.first()?;
        if root.kind != KIND_ARR_START
            || cached.is_empty()
            || bitmap.len() < cached.len().div_ceil(64)
        {
            return None;
        }
        let end = root.link as usize;
        if end == 0 || tape.get(end)?.kind != KIND_ARR_END {
            return None;
        }
        let mut array = ConstructionArray::new(&mut self.batch, cached.len() as u32);
        let mut index = 1;
        for i in 0..cached.len() {
            debug_assert!(index < end);
            let entry = &tape[index];
            let value = if bitmap[i / 64] & (1u64 << (i % 64)) != 0 {
                // This exact value may already have aliases and mutations.
                // Skip its source subtree without allocating a replacement.
                cached[i]
            } else {
                self.pos = entry.offset as usize;
                self.parse_value()
            };
            array.push(&mut self.batch, value);
            index = if matches!(entry.kind, KIND_OBJ_START | KIND_ARR_START) {
                entry.link as usize + 1
            } else {
                index + 1
            };
        }
        debug_assert_eq!(index, end);
        self.pos = tape[end].offset as usize + 1;
        Some(JSValue::object_ptr(array.finish(&self.batch).cast()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_cached_array_does_not_decode_the_replaced_subtree() {
        let skipped_key = b"cached_array_subtree_must_not_be_interned";
        let text = br#"[{"cached_array_subtree_must_not_be_interned":{"n":[1,2,3]}},{"x":1,"x":2},[true,null,"\ud800"],-0]"#;
        unsafe {
            let _suppress = crate::gc::GcSuppressScope::new();
            let scope = crate::gc::RuntimeHandleScope::new();
            let mut replacement = DirectParser::new_batched(br#"{"mutated":42}"#);
            let cached_value = replacement.parse_value();
            assert!(replacement.finish());
            let cached_root = scope.root_nanbox_u64(cached_value.bits());
            assert!(!crate::json::PARSE_KEY_CACHE
                .with(|cache| cache.borrow().contains_key(skipped_key.as_slice())));
            let tape = crate::json_tape::build_tape(text).unwrap();
            let mut parser = DirectParser::new_batched(text);
            let mut cached = [JSValue::undefined(); 4];
            cached[0] = JSValue::from_bits(cached_root.get_nanbox_u64());
            let result = parser
                .materialize_cached_array(&tape.entries, &cached, &[1])
                .expect("cached producer must run");
            assert!(parser.finish());
            let result = scope.root_nanbox_u64(result.bits());
            // Output equality alone would also pass after rebuilding and then
            // overwriting slot zero. Absence of this key proves it was skipped.
            assert!(!crate::json::PARSE_KEY_CACHE
                .with(|cache| cache.borrow().contains_key(skipped_key.as_slice())));
            let arr = JSValue::from_bits(result.get_nanbox_u64())
                .as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            assert_eq!(
                crate::array::js_array_get(arr, 0).bits(),
                cached_value.bits()
            );
            let output = crate::json::js_json_stringify(f64::from_bits(result.get_nanbox_u64()), 0);
            assert_eq!(
                crate::string::string_as_str(output),
                r#"[{"mutated":42},{"x":2},[true,null,"\ud800"],0]"#
            );
        }
    }

    #[test]
    fn json_cached_array_bitmap_boundaries_preserve_pointer_layout() {
        let text = format!("[{}]", vec!["1"; 130].join(","));
        unsafe {
            let _suppress = crate::gc::GcSuppressScope::new();
            let scope = crate::gc::RuntimeHandleScope::new();
            let string = crate::string::js_string_from_bytes(b"kept".as_ptr(), 4);
            let root = scope.root_nanbox_u64(JSValue::string_ptr(string).bits());
            let mut cached = vec![JSValue::undefined(); 130];
            for i in [0, 64, 129] {
                cached[i] = JSValue::from_bits(root.get_nanbox_u64());
            }
            let tape = crate::json_tape::build_tape(text.as_bytes()).unwrap();
            let mut parser = DirectParser::new_batched(text.as_bytes());
            let result = parser
                .materialize_cached_array(&tape.entries, &cached, &[1, 1, 2])
                .expect("cached producer must run");
            assert!(parser.finish());
            let result = scope.root_nanbox_u64(result.bits());
            let arr = JSValue::from_bits(result.get_nanbox_u64())
                .as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            assert_eq!((*arr).length, 130);
            assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 0);
            assert_eq!(
                crate::gc::test_layout_pointer_slot_count(arr as usize, 130),
                Some(3)
            );
            for i in 0..130 {
                let expected = if [0, 64, 129].contains(&i) {
                    root.get_nanbox_u64()
                } else {
                    JSValue::number(1.0).bits()
                };
                assert_eq!(crate::array::js_array_get(arr, i).bits(), expected);
            }
        }
    }

    #[test]
    fn json_cached_array_declines_before_consuming_input() {
        for text in ["[]", "[1]", r#"{"a":1}"#] {
            unsafe {
                let _suppress = crate::gc::GcSuppressScope::new();
                let tape = crate::json_tape::build_tape(text.as_bytes()).unwrap();
                let mut parser = DirectParser::new_batched(text.as_bytes());
                assert!(parser
                    .materialize_cached_array(&tape.entries, &[JSValue::undefined()], &[])
                    .is_none());
                assert_eq!(parser.pos, 0);
                parser.parse_value();
                assert!(parser.finish());
            }
        }
    }
}
