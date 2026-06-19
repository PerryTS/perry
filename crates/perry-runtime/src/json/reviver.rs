//! `JSON.parse(text, reviver)` — applies a user-supplied reviver function
//! to every property of the parsed value (post-order, root last).

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};

// ─── JSON.parse with reviver ────────────────────────────────────────────────

#[derive(Debug)]
enum JsonSourcePrimitive {
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
}

#[derive(Debug)]
struct JsonSourceField {
    key: Vec<u8>,
    value: JsonSourceNode,
}

#[derive(Debug)]
enum JsonSourceNode {
    Primitive {
        source: Vec<u8>,
        value: JsonSourcePrimitive,
    },
    Array {
        elements: Vec<JsonSourceNode>,
        original_bits: u64,
    },
    Object {
        fields: Vec<JsonSourceField>,
        original_bits: u64,
    },
}

struct JsonSourceParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> JsonSourceParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn parse(mut self) -> Option<JsonSourceNode> {
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.pos == self.input.len() {
            Some(value)
        } else {
            None
        }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    #[inline]
    fn consume(&mut self, ch: u8) -> bool {
        if self.peek() == Some(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Option<JsonSourceNode> {
        self.skip_whitespace();
        match self.peek()? {
            b'"' => self.parse_string_value(),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' => self.parse_literal(b"true", JsonSourcePrimitive::Bool(true)),
            b'f' => self.parse_literal(b"false", JsonSourcePrimitive::Bool(false)),
            b'n' => self.parse_literal(b"null", JsonSourcePrimitive::Null),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_literal(
        &mut self,
        literal: &[u8],
        value: JsonSourcePrimitive,
    ) -> Option<JsonSourceNode> {
        let end = self.pos.checked_add(literal.len())?;
        if self.input.get(self.pos..end)? != literal {
            return None;
        }
        self.pos = end;
        Some(JsonSourceNode::Primitive {
            source: literal.to_vec(),
            value,
        })
    }

    fn parse_number(&mut self) -> Option<JsonSourceNode> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let source = self.input.get(start..self.pos)?.to_vec();
        let number = std::str::from_utf8(&source).ok()?.parse::<f64>().ok()?;
        Some(JsonSourceNode::Primitive {
            source,
            value: JsonSourcePrimitive::Number(number),
        })
    }

    fn parse_string_value(&mut self) -> Option<JsonSourceNode> {
        let (source, decoded) = self.parse_string_token()?;
        Some(JsonSourceNode::Primitive {
            source,
            value: JsonSourcePrimitive::String(decoded),
        })
    }

    fn parse_array(&mut self) -> Option<JsonSourceNode> {
        self.consume(b'[');
        self.skip_whitespace();
        let mut elements = Vec::new();
        if self.consume(b']') {
            return Some(JsonSourceNode::Array {
                elements,
                original_bits: 0,
            });
        }
        loop {
            elements.push(self.parse_value()?);
            self.skip_whitespace();
            if self.consume(b',') {
                continue;
            }
            if !self.consume(b']') {
                return None;
            }
            break;
        }
        Some(JsonSourceNode::Array {
            elements,
            original_bits: 0,
        })
    }

    fn parse_object(&mut self) -> Option<JsonSourceNode> {
        self.consume(b'{');
        self.skip_whitespace();
        let mut fields: Vec<JsonSourceField> = Vec::new();
        if self.consume(b'}') {
            return Some(JsonSourceNode::Object {
                fields,
                original_bits: 0,
            });
        }
        loop {
            self.skip_whitespace();
            let (_, key) = self.parse_string_token()?;
            self.skip_whitespace();
            if !self.consume(b':') {
                return None;
            }
            let value = self.parse_value()?;
            if let Some(existing) = fields.iter_mut().find(|field| field.key == key) {
                existing.value = value;
            } else {
                fields.push(JsonSourceField { key, value });
            }
            self.skip_whitespace();
            if self.consume(b',') {
                continue;
            }
            if !self.consume(b'}') {
                return None;
            }
            break;
        }
        Some(JsonSourceNode::Object {
            fields,
            original_bits: 0,
        })
    }

    fn parse_string_token(&mut self) -> Option<(Vec<u8>, Vec<u8>)> {
        if !self.consume(b'"') {
            return None;
        }
        let token_start = self.pos - 1;
        let mut decoded = Vec::new();
        let mut segment_start = self.pos;
        loop {
            let ch = *self.input.get(self.pos)?;
            self.pos += 1;
            match ch {
                b'"' => {
                    let segment_end = self.pos - 1;
                    if segment_start < segment_end {
                        decoded.extend_from_slice(&self.input[segment_start..segment_end]);
                    }
                    let source = self.input[token_start..self.pos].to_vec();
                    return Some((source, decoded));
                }
                b'\\' => {
                    let segment_end = self.pos - 1;
                    if segment_start < segment_end {
                        decoded.extend_from_slice(&self.input[segment_start..segment_end]);
                    }
                    let esc = *self.input.get(self.pos)?;
                    self.pos += 1;
                    match esc {
                        b'"' => decoded.push(b'"'),
                        b'\\' => decoded.push(b'\\'),
                        b'/' => decoded.push(b'/'),
                        b'n' => decoded.push(b'\n'),
                        b'r' => decoded.push(b'\r'),
                        b't' => decoded.push(b'\t'),
                        b'b' => decoded.push(0x08),
                        b'f' => decoded.push(0x0C),
                        b'u' => {
                            let code = self.parse_hex_u16()?;
                            if (0xD800..=0xDBFF).contains(&code) {
                                if self.pos + 6 <= self.input.len()
                                    && self.input[self.pos] == b'\\'
                                    && self.input[self.pos + 1] == b'u'
                                {
                                    self.pos += 2;
                                    let low = self.parse_hex_u16()?;
                                    if (0xDC00..=0xDFFF).contains(&low) {
                                        let codepoint = 0x10000
                                            + ((code as u32 - 0xD800) << 10)
                                            + (low as u32 - 0xDC00);
                                        push_utf8_codepoint(&mut decoded, codepoint);
                                    }
                                }
                            } else if !(0xDC00..=0xDFFF).contains(&code) {
                                push_utf8_codepoint(&mut decoded, code as u32);
                            }
                        }
                        _ => decoded.push(esc),
                    }
                    segment_start = self.pos;
                }
                _ => {}
            }
        }
    }

    fn parse_hex_u16(&mut self) -> Option<u16> {
        let end = self.pos.checked_add(4)?;
        let mut value = 0u16;
        for &byte in self.input.get(self.pos..end)? {
            value = (value << 4) | hex_value(byte)? as u16;
        }
        self.pos = end;
        Some(value)
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn push_utf8_codepoint(out: &mut Vec<u8>, codepoint: u32) {
    if let Some(ch) = char::from_u32(codepoint) {
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }
}

fn parse_json_source_tree(input: &[u8]) -> Option<JsonSourceNode> {
    JsonSourceParser::new(input).parse()
}

/// Force-materialize a lazy-tape array (`PERRY_JSON_TAPE`) into a real
/// `ArrayHeader` tree and return a JSValue pointing at it. The reviver walk
/// below reads `length`/`capacity`/element f64s directly off the pointer — a
/// `LazyArrayHeader` has a different layout, so without this the walk reads
/// garbage and SIGSEGVs. Unlike `redirect_lazy_to_materialized` (stringify),
/// this forces materialization even when nothing has indexed the array yet.
/// No-op for non-lazy values. Refs #1424.
unsafe fn force_materialize_if_lazy(value: JSValue) -> JSValue {
    let bits = value.bits();
    if (bits >> 48) != 0x7FFD {
        return value;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return value;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return value;
    }
    let lazy = ptr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return value;
    }
    let materialized = crate::json_tape::force_materialize_lazy(lazy);
    if materialized.is_null() {
        return value;
    }
    JSValue::object_ptr(materialized as *mut u8)
}

unsafe fn pointer_value(bits: u64) -> f64 {
    f64::from_bits(POINTER_TAG | (bits & POINTER_MASK))
}

unsafe fn data_descriptor_value(value_handle: &crate::gc::RuntimeHandle<'_>) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let desc = crate::object::js_object_alloc(0, 4);
    let desc_handle = scope.root_raw_mut_ptr(desc);

    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let value_key_handle = scope.root_string_ptr(value_key);
    crate::object::js_object_set_field_by_name(
        desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        value_key_handle.get_raw_const_ptr::<StringHeader>(),
        value_handle.get_nanbox_f64(),
    );

    for (name, field_value) in [
        (b"writable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"enumerable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"configurable".as_slice(), f64::from_bits(TAG_TRUE)),
    ] {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_handle = scope.root_string_ptr(key);
        crate::object::js_object_set_field_by_name(
            desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            key_handle.get_raw_const_ptr::<StringHeader>(),
            field_value,
        );
    }
    pointer_value(desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64)
}

unsafe fn reflect_get_property(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
) -> JSValue {
    let result = crate::proxy::js_reflect_get(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        holder_handle.get_nanbox_f64(),
    );
    JSValue::from_bits(result.to_bits())
}

fn to_length(value: f64) -> usize {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() || number <= 0.0 {
        return 0;
    }
    if number.is_infinite() {
        return MAX_SAFE_INTEGER as usize;
    }
    number.floor().min(MAX_SAFE_INTEGER) as usize
}

unsafe fn json_is_array(value: f64) -> bool {
    let mut current = value;
    for _ in 0..64 {
        if crate::proxy::js_proxy_is_proxy(current) != 0 {
            let Some(target) = crate::proxy::js_proxy_checked_target_for_is_array(current) else {
                return false;
            };
            current = target;
            continue;
        }
        return crate::array::js_array_is_array(current).to_bits() == TAG_TRUE;
    }
    false
}

unsafe fn json_is_object(value: f64) -> bool {
    if crate::proxy::js_proxy_is_proxy(value) != 0 {
        let _ = crate::proxy::js_proxy_checked_target(value);
        return true;
    }
    if let Some(ptr) = extract_pointer(value.to_bits()) {
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return false;
        }
        return gc_obj_type(ptr) == crate::gc::GC_TYPE_OBJECT;
    }
    false
}

unsafe fn string_header_bytes<'a>(ptr: *const StringHeader) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return None;
    }
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    Some(std::slice::from_raw_parts(data, (*ptr).byte_len as usize))
}

fn number_matches_source(value: JSValue, source_number: f64) -> bool {
    let current = if value.is_int32() {
        value.as_int32() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        return false;
    };
    if source_number == 0.0 && current == 0.0 {
        return source_number.is_sign_negative() == current.is_sign_negative();
    }
    current == source_number
}

fn primitive_matches_source(value_bits: u64, source_value: &JsonSourcePrimitive) -> bool {
    let value = JSValue::from_bits(value_bits);
    match source_value {
        JsonSourcePrimitive::Null => value.is_null(),
        JsonSourcePrimitive::Bool(expected) => value.is_bool() && value.as_bool() == *expected,
        JsonSourcePrimitive::Number(expected) => number_matches_source(value, *expected),
        JsonSourcePrimitive::String(expected) => {
            if !value.is_any_string() {
                return false;
            }
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let Some((ptr, len)) =
                crate::string::str_bytes_from_jsvalue(f64::from_bits(value_bits), &mut scratch)
            else {
                return false;
            };
            if len as usize != expected.len() {
                return false;
            }
            if len == 0 {
                return true;
            }
            unsafe { std::slice::from_raw_parts(ptr, len as usize) == expected.as_slice() }
        }
    }
}

fn primitive_source_for_value(source: Option<&JsonSourceNode>, value_bits: u64) -> Option<&[u8]> {
    match source {
        Some(JsonSourceNode::Primitive { source, value })
            if primitive_matches_source(value_bits, value) =>
        {
            Some(source.as_slice())
        }
        _ => None,
    }
}

unsafe fn create_reviver_context(
    value_handle: &crate::gc::RuntimeHandle<'_>,
    source: Option<&JsonSourceNode>,
) -> f64 {
    let source_bytes = primitive_source_for_value(source, value_handle.get_nanbox_u64());
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, u32::from(source_bytes.is_some()));
    let obj_handle = scope.root_raw_mut_ptr(obj);

    if let Some(source_bytes) = source_bytes {
        let source_value = js_string_from_bytes(source_bytes.as_ptr(), source_bytes.len() as u32);
        let source_value_handle = scope.root_string_ptr(source_value);
        let source_key = js_string_from_bytes(b"source".as_ptr(), 6);
        let source_key_handle = scope.root_string_ptr(source_key);
        crate::object::js_object_set_field_by_name(
            obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            source_key_handle.get_raw_const_ptr::<StringHeader>(),
            nanbox_string_f64(source_value_handle.get_raw_const_ptr::<StringHeader>()),
        );
    }

    pointer_value(obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64)
}

unsafe fn annotate_source_tree(node: &mut JsonSourceNode, value: JSValue) {
    let value = force_materialize_if_lazy(value);
    let Some(ptr) = extract_pointer(value.bits()) else {
        return;
    };
    match (node, gc_obj_type(ptr)) {
        (
            JsonSourceNode::Array {
                elements,
                original_bits,
            },
            crate::gc::GC_TYPE_ARRAY,
        ) => {
            *original_bits = value.bits();
            let arr = ptr as *const crate::ArrayHeader;
            let len = (*arr).length as usize;
            for (index, child) in elements.iter_mut().enumerate().take(len) {
                let child_value = crate::array::js_array_get(arr, index as u32);
                annotate_source_tree(child, child_value);
            }
        }
        (
            JsonSourceNode::Object {
                fields,
                original_bits,
            },
            crate::gc::GC_TYPE_OBJECT,
        ) => {
            *original_bits = value.bits();
            let obj = ptr as *const crate::ObjectHeader;
            for field in fields {
                let key = cached_parse_key_ptr(&field.key);
                let child_value = crate::object::js_object_get_field_by_name(obj, key);
                annotate_source_tree(&mut field.value, child_value);
            }
        }
        _ => {}
    }
}

fn array_child_source(
    source: Option<&JsonSourceNode>,
    parent_bits: u64,
    index: usize,
) -> Option<&JsonSourceNode> {
    match source {
        Some(JsonSourceNode::Array {
            elements,
            original_bits,
        }) if *original_bits == parent_bits => elements.get(index),
        _ => None,
    }
}

unsafe fn object_child_source(
    source: Option<&JsonSourceNode>,
    parent_bits: u64,
    key: *const StringHeader,
) -> Option<&JsonSourceNode> {
    match source {
        Some(JsonSourceNode::Object {
            fields,
            original_bits,
        }) if *original_bits == parent_bits => {
            let key_bytes = string_header_bytes(key)?;
            fields
                .iter()
                .find(|field| field.key.as_slice() == key_bytes)
                .map(|field| &field.value)
        }
        _ => None,
    }
}

unsafe fn holder_ptr_from_bits(bits: u64) -> *mut crate::ObjectHeader {
    (bits & POINTER_MASK) as *mut crate::ObjectHeader
}

unsafe fn delete_property_or_keep(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let _ = crate::proxy::js_reflect_delete(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
    );
}

unsafe fn create_data_property_or_keep(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    value_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let descriptor = data_descriptor_value(value_handle);
    let descriptor_handle = scope.root_nanbox_f64(descriptor);
    let _ = crate::proxy::js_reflect_define_property(
        holder_handle.get_nanbox_f64(),
        key_handle.get_nanbox_f64(),
        descriptor_handle.get_nanbox_f64(),
    );
}

unsafe fn apply_internalized_child(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    child: JSValue,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let child_handle = scope.root_nanbox_u64(child.bits());
    if child_handle.get_nanbox_u64() == TAG_UNDEFINED {
        delete_property_or_keep(holder_handle, key_handle);
    } else {
        create_data_property_or_keep(holder_handle, key_handle, &child_handle);
    }
}

unsafe fn internalize_array(
    value_handle: &crate::gc::RuntimeHandle<'_>,
    reviver: *const crate::closure::ClosureHeader,
    source: Option<&JsonSourceNode>,
) {
    let reviver_scope = crate::gc::RuntimeHandleScope::new();
    let reviver_handle = reviver_scope.root_raw_const_ptr(reviver);
    let length_key = js_string_from_bytes(b"length".as_ptr(), 6);
    let length_key_handle = reviver_scope.root_nanbox_f64(nanbox_string_f64(length_key));
    let length_value = reflect_get_property(value_handle, &length_key_handle);
    let len = to_length(f64::from_bits(length_value.bits()));
    for i in 0..len {
        let iteration_scope = crate::gc::RuntimeHandleScope::new();
        let idx = i.to_string();
        let key = js_string_from_bytes(idx.as_ptr(), idx.len() as u32);
        let key_handle = iteration_scope.root_string_ptr(key);
        let key_value = nanbox_string_f64(key_handle.get_raw_const_ptr::<StringHeader>());
        let key_value_handle = iteration_scope.root_nanbox_f64(key_value);
        let child_source = array_child_source(source, value_handle.get_nanbox_u64(), i);
        let child = internalize_json_property(
            JSValue::from_bits(value_handle.get_nanbox_u64()),
            key_value_handle.get_nanbox_f64(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
            child_source,
        );
        apply_internalized_child(value_handle, &key_value_handle, child);
    }
}

unsafe fn enumerable_keys_for_internalize(
    value_handle: &crate::gc::RuntimeHandle<'_>,
) -> *mut crate::ArrayHeader {
    let value = value_handle.get_nanbox_f64();
    let keys_value = if crate::proxy::js_proxy_is_proxy(value) != 0 {
        crate::proxy::js_proxy_own_keys_for_json(value)
    } else {
        let keys = crate::object::js_object_keys_value(value);
        f64::from_bits(POINTER_TAG | ((keys as u64) & POINTER_MASK))
    };
    (keys_value.to_bits() & POINTER_MASK) as *mut crate::ArrayHeader
}

unsafe fn internalize_object(
    value_handle: &crate::gc::RuntimeHandle<'_>,
    reviver: *const crate::closure::ClosureHeader,
    source: Option<&JsonSourceNode>,
) {
    let reviver_scope = crate::gc::RuntimeHandleScope::new();
    let reviver_handle = reviver_scope.root_raw_const_ptr(reviver);
    let keys = enumerable_keys_for_internalize(value_handle);
    let scope = crate::gc::RuntimeHandleScope::new();
    let keys_handle = scope.root_raw_mut_ptr(keys);
    let len = crate::array::js_array_length(keys_handle.get_raw_mut_ptr::<crate::ArrayHeader>());
    for i in 0..len {
        let iteration_scope = crate::gc::RuntimeHandleScope::new();
        let keys = keys_handle.get_raw_mut_ptr::<crate::ArrayHeader>();
        let key_value = crate::array::js_array_get(keys, i);
        let key_value_handle = iteration_scope.root_nanbox_u64(key_value.bits());
        let key_ptr = crate::value::js_get_string_pointer_unified(key_value_handle.get_nanbox_f64())
            as *const StringHeader;
        if key_ptr.is_null() {
            continue;
        }
        let key_handle = iteration_scope.root_string_ptr(key_ptr);
        let key_value = nanbox_string_f64(key_handle.get_raw_const_ptr::<StringHeader>());
        let key_value_handle = iteration_scope.root_nanbox_f64(key_value);
        let child_source = object_child_source(
            source,
            value_handle.get_nanbox_u64(),
            key_handle.get_raw_const_ptr::<StringHeader>(),
        );
        let child = internalize_json_property(
            JSValue::from_bits(value_handle.get_nanbox_u64()),
            key_value_handle.get_nanbox_f64(),
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
            child_source,
        );
        apply_internalized_child(value_handle, &key_value_handle, child);
    }
}

unsafe fn call_reviver(
    holder_handle: &crate::gc::RuntimeHandle<'_>,
    key_handle: &crate::gc::RuntimeHandle<'_>,
    value_handle: &crate::gc::RuntimeHandle<'_>,
    reviver: *const crate::closure::ClosureHeader,
    source: Option<&JsonSourceNode>,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let context_arg = create_reviver_context(value_handle, source);
    let context_handle = scope.root_nanbox_f64(context_arg);
    let holder_arg = holder_handle.get_nanbox_f64();
    let key_arg = key_handle.get_nanbox_f64();
    let value_arg = value_handle.get_nanbox_f64();
    let prev_this = crate::object::js_implicit_this_set(holder_arg);
    let result =
        crate::js_closure_call3(reviver, key_arg, value_arg, context_handle.get_nanbox_f64());
    crate::object::js_implicit_this_set(prev_this);
    let result_bits = result.to_bits();
    let revived_bits = if result_bits == value_arg.to_bits() {
        value_handle.get_nanbox_u64()
    } else if result_bits == key_arg.to_bits() {
        key_handle.get_nanbox_u64()
    } else if result_bits == holder_arg.to_bits() {
        holder_handle.get_nanbox_u64()
    } else if result_bits == context_arg.to_bits() {
        context_handle.get_nanbox_u64()
    } else {
        result_bits
    };
    JSValue::from_bits(revived_bits)
}

unsafe fn internalize_json_property(
    holder: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
    source: Option<&JsonSourceNode>,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder_handle = scope.root_nanbox_u64(holder.bits());
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let key_handle = scope.root_nanbox_f64(key_f64);
    let value = reflect_get_property(&holder_handle, &key_handle);
    let value = force_materialize_if_lazy(value);
    let value_handle = scope.root_nanbox_u64(value.bits());

    if json_is_array(value_handle.get_nanbox_f64()) {
        internalize_array(
            &value_handle,
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
            source,
        );
    } else if json_is_object(value_handle.get_nanbox_f64()) {
        internalize_object(
            &value_handle,
            reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
            source,
        );
    }

    call_reviver(
        &holder_handle,
        &key_handle,
        &value_handle,
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        source,
    )
}

/// Apply reviver to a parsed JSON value through the same root-holder wrapper
/// used by `JSON.parse(text, reviver)`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) unsafe fn apply_reviver(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    apply_reviver_with_source(value, key_f64, reviver, None)
}

unsafe fn apply_reviver_with_source(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
    mut source: Option<&mut JsonSourceNode>,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let wrapper = crate::object::js_object_alloc(0, 1);
    let wrapper_handle = scope.root_raw_mut_ptr(wrapper);
    let reviver_handle = scope.root_raw_const_ptr(reviver);
    let key_handle = scope.root_nanbox_f64(key_f64);
    let key_ptr = crate::value::js_get_string_pointer_unified(key_handle.get_nanbox_f64())
        as *const StringHeader;
    let key_ptr_handle = scope.root_string_ptr(key_ptr);
    let value = force_materialize_if_lazy(value);
    let value_handle = scope.root_nanbox_u64(value.bits());
    if let Some(source_node) = source.as_deref_mut() {
        annotate_source_tree(
            source_node,
            JSValue::from_bits(value_handle.get_nanbox_u64()),
        );
    }
    crate::object::js_object_set_field_by_name(
        wrapper_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key_ptr_handle.get_raw_const_ptr::<StringHeader>(),
        value_handle.get_nanbox_f64(),
    );
    internalize_json_property(
        JSValue::object_ptr(wrapper_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as *mut u8),
        key_handle.get_nanbox_f64(),
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        source.as_deref(),
    )
}

#[cfg(test)]
pub(crate) unsafe fn test_apply_reviver_for_value(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    apply_reviver(value, key_f64, reviver)
}

/// JSON.parse(text, reviver) — parse JSON with a reviver function.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_with_reviver(
    text_ptr: *const StringHeader,
    reviver_ptr: i64,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let text_handle = scope.root_string_ptr(text_ptr);
    let reviver = reviver_ptr as *const crate::closure::ClosureHeader;
    let reviver_handle = scope.root_raw_const_ptr(reviver);

    // First, parse normally
    let parsed = js_json_parse(text_handle.get_raw_const_ptr::<StringHeader>());
    let parsed_handle = scope.root_nanbox_u64(parsed.bits());

    if reviver.is_null() || (reviver_ptr as u64) < 0x1000 {
        return JSValue::from_bits(parsed_handle.get_nanbox_u64());
    }

    let text = text_handle.get_raw_const_ptr::<StringHeader>();
    let len = (*text).byte_len as usize;
    let data_ptr = (text as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    let mut source_tree = parse_json_source_tree(bytes);

    // Apply reviver starting from root
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_handle = scope.root_nanbox_f64(nanbox_string_f64(empty_str));
    apply_reviver_with_source(
        JSValue::from_bits(parsed_handle.get_nanbox_u64()),
        empty_key_handle.get_nanbox_f64(),
        reviver_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>(),
        source_tree.as_mut(),
    )
}
