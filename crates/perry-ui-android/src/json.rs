//! JSON handling for Android — copied from perry-stdlib/src/framework/json.rs
//!
//! perry-stdlib can't cross-compile for Android (OpenSSL dependency), so we
//! include the essential JSON functions directly. These replace the no-op stubs
//! in stdlib_stubs.rs.

use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_set_field, js_object_set_keys,
    js_string_from_bytes, JSValue, StringHeader,
};
use std::fmt::Write as FmtWrite;

// ─── Zero-copy string access ──────────────────────────────────────────────────

#[inline]
unsafe fn str_from_header<'a>(ptr: *const StringHeader) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(std::str::from_utf8_unchecked(bytes))
}

unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    str_from_header(ptr).map(|s| s.to_string())
}

// ─── Direct JSON parser ────────────────────────────────────────────────────────

struct DirectParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> DirectParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    #[inline]
    fn advance(&mut self) {
        self.pos += 1;
    }

    #[inline]
    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    #[inline]
    fn expect(&mut self, ch: u8) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(ch) {
            self.advance();
            true
        } else {
            false
        }
    }

    unsafe fn parse_value(&mut self) -> JSValue {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string_value(),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_true(),
            Some(b'f') => self.parse_false(),
            Some(b'n') => self.parse_null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => JSValue::null(),
        }
    }

    unsafe fn parse_string_value(&mut self) -> JSValue {
        if let Some(s) = self.parse_string_bytes() {
            let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            JSValue::string_ptr(ptr)
        } else {
            JSValue::null()
        }
    }

    fn parse_string_bytes(&mut self) -> Option<Vec<u8>> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.advance();

        let mut result = Vec::new();
        loop {
            if self.pos >= self.input.len() {
                return None;
            }
            let ch = self.input[self.pos];
            self.pos += 1;
            match ch {
                b'"' => return Some(result),
                b'\\' => {
                    if self.pos >= self.input.len() {
                        return None;
                    }
                    let esc = self.input[self.pos];
                    self.pos += 1;
                    match esc {
                        b'"' => result.push(b'"'),
                        b'\\' => result.push(b'\\'),
                        b'/' => result.push(b'/'),
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'b' => result.push(0x08),
                        b'f' => result.push(0x0C),
                        b'u' => {
                            if self.pos + 4 > self.input.len() {
                                return None;
                            }
                            let hex =
                                std::str::from_utf8(&self.input[self.pos..self.pos + 4]).ok()?;
                            let code = u16::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            if (0xD800..=0xDBFF).contains(&code) {
                                if self.pos + 6 <= self.input.len()
                                    && self.input[self.pos] == b'\\'
                                    && self.input[self.pos + 1] == b'u'
                                {
                                    let hex2 = std::str::from_utf8(
                                        &self.input[self.pos + 2..self.pos + 6],
                                    )
                                    .ok()?;
                                    let low = u16::from_str_radix(hex2, 16).ok()?;
                                    self.pos += 6;
                                    let codepoint = 0x10000
                                        + ((code as u32 - 0xD800) << 10)
                                        + (low as u32 - 0xDC00);
                                    if let Some(c) = char::from_u32(codepoint) {
                                        let mut buf = [0u8; 4];
                                        let s = c.encode_utf8(&mut buf);
                                        result.extend_from_slice(s.as_bytes());
                                    }
                                }
                            } else {
                                if let Some(c) = char::from_u32(code as u32) {
                                    let mut buf = [0u8; 4];
                                    let s = c.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                }
                            }
                        }
                        _ => result.push(esc),
                    }
                }
                _ => result.push(ch),
            }
        }
    }

    unsafe fn parse_object(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let mut pairs: Vec<(Vec<u8>, JSValue)> = Vec::new();

        if self.peek() == Some(b'}') {
            self.advance();
            let js_obj = js_object_alloc(0, 0);
            let keys_arr = js_array_alloc(0);
            js_object_set_keys(js_obj, keys_arr);
            return JSValue::object_ptr(js_obj as *mut u8);
        }

        loop {
            self.skip_whitespace();
            let key = match self.parse_string_bytes() {
                Some(k) => k,
                None => break,
            };

            if !self.expect(b':') {
                break;
            }

            let value = self.parse_value();
            pairs.push((key, value));

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b'}');

        let count = pairs.len();
        let js_obj = js_object_alloc(0, count as u32);
        let keys_arr = js_array_alloc(count as u32);

        for (idx, (key, value)) in pairs.into_iter().enumerate() {
            let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
            js_array_push(keys_arr, JSValue::string_ptr(key_ptr));
            js_object_set_field(js_obj, idx as u32, value);
        }
        js_object_set_keys(js_obj, keys_arr);
        JSValue::object_ptr(js_obj as *mut u8)
    }

    unsafe fn parse_array(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let js_arr = js_array_alloc(16);

        if self.peek() == Some(b']') {
            self.advance();
            return JSValue::object_ptr(js_arr as *mut u8);
        }

        loop {
            let value = self.parse_value();
            js_array_push(js_arr, value);

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b']');
        JSValue::object_ptr(js_arr as *mut u8)
    }

    unsafe fn parse_number(&mut self) -> JSValue {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance();
        }
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        let num_str = std::str::from_utf8_unchecked(&self.input[start..self.pos]);
        let value: f64 = num_str.parse().unwrap_or(0.0);
        JSValue::number(value)
    }

    unsafe fn parse_true(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"true" {
            self.pos += 4;
            JSValue::bool(true)
        } else {
            JSValue::null()
        }
    }

    unsafe fn parse_false(&mut self) -> JSValue {
        if self.pos + 5 <= self.input.len() && &self.input[self.pos..self.pos + 5] == b"false" {
            self.pos += 5;
            JSValue::bool(false)
        } else {
            JSValue::null()
        }
    }

    unsafe fn parse_null(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"null" {
            self.pos += 4;
        }
        JSValue::null()
    }
}

// ─── NaN-boxing constants ─────────────────────────────────────────────────────

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

const TYPE_UNKNOWN: u32 = 0;
const TYPE_OBJECT: u32 = 1;
const TYPE_ARRAY: u32 = 2;

#[inline]
fn is_raw_pointer(bits: u64) -> bool {
    let exponent = (bits >> 52) & 0x7FF;
    let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
    let sign = bits >> 63;
    exponent == 0 && mantissa != 0 && sign == 0
}

#[inline]
unsafe fn extract_pointer(bits: u64) -> Option<*const u8> {
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == POINTER_TAG {
        Some((bits & POINTER_MASK) as *const u8)
    } else if is_raw_pointer(bits) {
        Some(bits as *const u8)
    } else {
        None
    }
}

#[inline]
unsafe fn is_object_pointer(ptr: *const u8) -> bool {
    let obj = ptr as *const perry_runtime::ObjectHeader;
    let potential_keys_ptr = (*obj).keys_array as u64;
    let top_16_bits = potential_keys_ptr >> 48;
    let is_likely_heap_pointer = top_16_bits == 0 || top_16_bits == 1;
    let looks_like_valid_pointer =
        is_likely_heap_pointer && potential_keys_ptr > 0x10000 && (potential_keys_ptr & 0x7) == 0;

    if looks_like_valid_pointer {
        let keys_arr = (*obj).keys_array;
        let keys_len = (*keys_arr).length;
        let keys_cap = (*keys_arr).capacity;
        let field_count = (*obj).field_count;
        keys_len <= keys_cap
            && keys_len > 0
            && keys_cap < 1000
            && field_count == keys_len
            && field_count < 1000
    } else {
        false
    }
}

#[inline]
unsafe fn write_number(buf: &mut String, value: f64) {
    if value.is_nan() || value.is_infinite() {
        buf.push_str("null");
    } else if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(value as i64));
    } else {
        let mut ryu_buf = ryu::Buffer::new();
        buf.push_str(ryu_buf.format(value));
    }
}

#[inline]
unsafe fn write_escaped_string(buf: &mut String, s: &str) {
    buf.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let escape = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0..=0x1f => {
                if start < i {
                    buf.push_str(&s[start..i]);
                }
                let _ = write!(buf, "\\u{:04x}", b);
                start = i + 1;
                continue;
            }
            _ => None,
        };
        if let Some(esc) = escape {
            if start < i {
                buf.push_str(&s[start..i]);
            }
            buf.push_str(esc);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        buf.push_str(&s[start..]);
    }
    buf.push('"');
}

unsafe fn stringify_value(value: f64, type_hint: u32, buf: &mut String) {
    let bits: u64 = value.to_bits();

    if bits == TAG_NULL {
        buf.push_str("null");
        return;
    }
    if bits == TAG_TRUE {
        buf.push_str("true");
        return;
    }
    if bits == TAG_FALSE {
        buf.push_str("false");
        return;
    }

    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_OBJECT {
            stringify_object(ptr, buf);
            return;
        }
        if type_hint == TYPE_ARRAY {
            stringify_array(ptr, buf);
            return;
        }
        if is_object_pointer(ptr) {
            stringify_object(ptr, buf);
        } else {
            let arr = ptr as *const perry_runtime::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 {
                    stringify_array(ptr, buf);
                    return;
                }
            }
            let str_ptr = ptr as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        }
        return;
    }

    write_number(buf, value);
}

unsafe fn stringify_object(ptr: *const u8, buf: &mut String) {
    let obj = ptr as *const perry_runtime::ObjectHeader;
    let num_fields = (*obj).field_count;
    buf.push('{');

    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements = (keys_arr as *const u8)
        .add(std::mem::size_of::<perry_runtime::ArrayHeader>())
        as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<perry_runtime::ObjectHeader>()) as *const f64;

    for f in 0..num_fields {
        if f > 0 {
            buf.push(',');
        }
        if (f as u32) < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            if let Some(key_str) = str_from_header(key_ptr) {
                buf.push('"');
                buf.push_str(key_str);
                buf.push_str("\":");
            } else {
                let _ = write!(buf, "\"field{}\":", f);
            }
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }
        let field_val = *fields_ptr.add(f as usize);
        stringify_value(field_val, TYPE_UNKNOWN, buf);
    }
    buf.push('}');
}

unsafe fn stringify_array(ptr: *const u8, buf: &mut String) {
    let arr = ptr as *const perry_runtime::ArrayHeader;
    let len = (*arr).length;
    let elements =
        (ptr as *const u8).add(std::mem::size_of::<perry_runtime::ArrayHeader>()) as *const f64;

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;

        if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_bits == TAG_NULL {
            buf.push_str("null");
        } else if elem_bits == TAG_TRUE {
            buf.push_str("true");
        } else if elem_bits == TAG_FALSE {
            buf.push_str("false");
        } else if elem_tag == POINTER_TAG || is_raw_pointer(elem_bits) {
            let elem_ptr = if elem_tag == POINTER_TAG {
                (elem_bits & POINTER_MASK) as *const u8
            } else {
                elem_bits as *const u8
            };
            if is_object_pointer(elem_ptr) {
                stringify_object(elem_ptr, buf);
            } else {
                let arr_elem = elem_ptr as *const perry_runtime::ArrayHeader;
                let arr_len = (*arr_elem).length;
                let arr_cap = (*arr_elem).capacity;
                if arr_len <= arr_cap && arr_cap > 0 && arr_cap < 10000 {
                    stringify_array(elem_ptr, buf);
                } else {
                    let str_ptr = elem_ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
            }
        } else {
            write_number(buf, elem);
        }
    }
    buf.push(']');
}

#[inline]
unsafe fn estimate_json_size(value: f64, type_hint: u32) -> usize {
    let bits = value.to_bits();
    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_ARRAY || (!is_object_pointer(ptr) && type_hint != TYPE_OBJECT) {
            let arr = ptr as *const perry_runtime::ArrayHeader;
            let len = (*arr).length as usize;
            return (len * 300).max(256);
        }
        if type_hint == TYPE_OBJECT || is_object_pointer(ptr) {
            let obj = ptr as *const perry_runtime::ObjectHeader;
            let fields = (*obj).field_count as usize;
            return (fields * 200).max(256);
        }
    }
    4096
}

// ─── Exported FFI functions ───────────────────────────────────────────────────
// js_json_* functions are now provided by perry-runtime/json.rs
