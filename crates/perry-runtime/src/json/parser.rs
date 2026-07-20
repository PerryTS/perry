//! Direct recursive-descent JSON parser used by `JSON.parse()`.
//!
//! Builds Perry `JSValue`s directly (no intermediate AST). Includes a
//! zero-copy fast path for unescaped string values and an "object-shape
//! hint" specialization used by `js_json_parse_typed_array`.

use super::*;
use crate::{
    array::{note_array_slot_layout_only, ArrayHeader},
    js_array_alloc, js_array_push, js_string_from_bytes, JSValue, StringHeader,
};

// ─── Direct JSON parser ────────────────────────────────────────────────────────

/// Result of parsing a JSON string: either a zero-copy borrow from the
/// input buffer (no escapes) or an owned allocation (had escape sequences).
pub(crate) enum ParsedStr<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> ParsedStr<'a> {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            ParsedStr::Borrowed(s) => s,
            ParsedStr::Owned(v) => v,
        }
    }
}

/// Issue #179 typed-parse plan, Step 1b. Pre-computed shape for
/// `JSON.parse<T[]>(blob)` where T is an object type with a known
/// field list. Built once per typed-parse call from the codegen-
/// emitted packed-keys bytes; reused for every record in the array.
///
/// The key contract: `expected_keys[i].bytes == <field name at index i>`.
/// When JSON fields arrive in declared order (the common case for
/// machine-generated JSON, including stringify output), the hot loop
/// just memcmp's `key_bytes` against `expected_keys[idx]` and writes
/// directly to `fields[idx]`, skipping the `PARSE_KEY_CACHE` hash
/// lookup AND the transition-cache dance inside
/// `js_object_set_field_by_name`.
///
/// Out-of-order fields and fields not in the shape fall through to
/// the generic path (same semantics as untyped parse).
pub(crate) struct ObjectShapeHint {
    /// Pre-interned key pointers in declared field order. Pointers
    /// are held alive by PARSE_KEY_CACHE + scan_parse_roots.
    pub(crate) expected_keys: Vec<*const StringHeader>,
    /// Pre-built keys_array that each parsed record's ObjectHeader
    /// points to. Built via `js_build_class_keys_array`, so the
    /// shape cache + scan_shape_cache_roots keeps it alive.
    pub(crate) keys_array: *mut crate::array::ArrayHeader,
    /// Number of fields in the declared shape — used as the object's
    /// pre-allocated field count.
    pub(crate) field_count: u32,
}

pub(crate) struct DirectParser<'a> {
    input: &'a [u8],
    pos: usize,
    /// Issue #179 typed-parse: if Some, the top-level value is
    /// expected to be `Array<Object>` matching this shape. Each
    /// record uses the fast path; mismatches silently fall through
    /// to the generic field-setting logic.
    shape: Option<ObjectShapeHint>,
    /// Per-parse one-entry shape cache for homogeneous object arrays.
    /// `parse_shape_keys_array` already has a thread-local cache, but
    /// repeatedly entering TLS + RefCell for every object is visible on
    /// 5k-record JSON feeds. Most direct-parser objects repeat one shape,
    /// so keep the last <=8-key shape in the parser itself.
    hot_shape_len: usize,
    hot_shape_keys: [*const StringHeader; 8],
    hot_shape_array: *mut ArrayHeader,
}

impl<'a> DirectParser<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            shape: None,
            hot_shape_len: 0,
            hot_shape_keys: [std::ptr::null(); 8],
            hot_shape_array: std::ptr::null_mut(),
        }
    }

    pub(crate) fn with_shape(input: &'a [u8], shape: ObjectShapeHint) -> Self {
        Self {
            input,
            pos: 0,
            shape: Some(shape),
            hot_shape_len: 0,
            hot_shape_keys: [std::ptr::null(); 8],
            hot_shape_array: std::ptr::null_mut(),
        }
    }

    #[inline]
    unsafe fn parse_shape_keys_array_hot(
        &mut self,
        keys: &[*const StringHeader],
    ) -> *mut ArrayHeader {
        if keys.len() <= self.hot_shape_keys.len()
            && keys.len() == self.hot_shape_len
            && !self.hot_shape_array.is_null()
            && self.hot_shape_keys[..self.hot_shape_len]
                .iter()
                .zip(keys.iter())
                .all(|(a, b)| std::ptr::eq(*a, *b))
        {
            return self.hot_shape_array;
        }

        let keys_array = parse_shape_keys_array(keys);
        if keys.len() <= self.hot_shape_keys.len() {
            self.hot_shape_len = keys.len();
            self.hot_shape_keys[..keys.len()].copy_from_slice(keys);
            self.hot_shape_array = keys_array;
        }
        keys_array
    }

    #[inline(always)]
    unsafe fn array_push_parse_fast(
        &self,
        arr: *mut ArrayHeader,
        value: JSValue,
    ) -> *mut ArrayHeader {
        let length = (*arr).length;
        if length < (*arr).capacity {
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
            let value_bits = value.bits();
            let slot = elements_ptr.add(length as usize);
            // GC_STORE_AUDIT(INIT): JSON.parse suppresses GC and notes layout for same-parse arrays below.
            std::ptr::write(slot, value_bits);
            // JSON.parse suppresses GC and writes only into arrays allocated
            // by the same parse, so a generational write barrier is redundant.
            // Keep the layout note so tracing still sees the element slot.
            note_array_slot_layout_only(arr, length as usize, value_bits);
            (*arr).length = length + 1;
            arr
        } else {
            js_array_push(arr, value)
        }
    }

    #[inline]
    pub(crate) fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    #[inline]
    pub(crate) fn advance(&mut self) {
        self.pos += 1;
    }

    #[inline]
    pub(crate) fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    /// After parsing the top-level value, returns `true` if any
    /// non-whitespace input remains. JSON.parse must reject such trailing
    /// tokens (`JSON.parse("{}x")`, `JSON.parse("1 2")`) with a `SyntaxError`;
    /// trailing whitespace (`"{}\n"`) is allowed.
    #[inline]
    pub(crate) fn has_trailing_content(&mut self) -> bool {
        self.skip_whitespace();
        self.pos < self.input.len()
    }

    #[inline]
    pub(crate) fn expect(&mut self, ch: u8) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(ch) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(crate) unsafe fn parse_value(&mut self) -> JSValue {
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

    pub(crate) unsafe fn parse_string_value(&mut self) -> JSValue {
        if let Some(s) = self.parse_string_bytes() {
            let b = s.as_bytes();
            // v0.5.216 SSO Step 2: emit inline SSO for values of
            // length ≤ SHORT_STRING_MAX_LEN (5 bytes). Zero heap
            // allocation on the short-string hot path. Consumer
            // arms for this representation landed in v0.5.213-215
            // (equality, comparison, typeof, length, stringify,
            // PropertyGet codegen, Array.join).
            //
            // Measured at flip (bench_sso_strings: 20k records × 4
            // short strings, 30 iters): direct-only 290 ms / 123 MB
            // → direct+SSO 150 ms / 76 MB (1.9× faster, 38% less
            // RSS). Main JSON benches also improve modestly on the
            // direct-forced path (7-12% time, 2-5% RSS).
            //
            // `PERRY_SSO_FORCE` env var retained as a no-op kept
            // alive for release-note compatibility — any value
            // still falls through to the unconditional SSO emit.
            if let Some(sso) = JSValue::try_short_string(b) {
                return sso;
            }
            // ASCII fast path: skip `compute_utf16_len`'s byte scan
            // (which `js_string_from_bytes` runs unconditionally) when
            // every byte is < 0x80. Most real-world JSON payloads —
            // user names, emails, ISO timestamps, slugs — are pure
            // ASCII; the standalone `is_ascii()` check is vectorised
            // (16 B/it on aarch64 NEON) so it costs ~1 ns/byte and
            // saves the equivalent walk inside `compute_utf16_len`
            // plus the conditional widening for non-ASCII counters.
            let ptr = if b.is_ascii() {
                crate::string::js_string_from_ascii_bytes(b.as_ptr(), b.len() as u32)
            } else {
                js_string_from_bytes(b.as_ptr(), b.len() as u32)
            };
            JSValue::string_ptr(ptr)
        } else {
            JSValue::null()
        }
    }

    /// Zero-copy fast path: if the string has no escape sequences,
    /// return a direct slice into the input buffer. Falls back to
    /// `parse_string_bytes_slow` for strings containing `\`.
    ///
    /// Issue #179 tier 1 #3: scans for `"` or `\` 16 bytes at a time
    /// using NEON (aarch64) or SSE2 (x86_64) when available, scalar
    /// fallback otherwise. On `bench_json_roundtrip` the per-record
    /// strings are 5-16 bytes so most iterations hit the SIMD path
    /// exactly once before the scalar tail handles the boundary.
    pub(crate) fn parse_string_bytes(&mut self) -> Option<ParsedStr<'a>> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.advance();
        let start = self.pos;

        // SIMD-accelerated scan for `"` or `\`. On match, fall through
        // to the scalar loop which positions `self.pos` exactly.
        if let Some(hit) = find_string_terminator(&self.input[self.pos..]) {
            // `hit` is the offset within the remaining slice of the
            // first `"` or `\`. If it's `"`, we're done; if `\`, slow
            // path picks up from the current position.
            self.pos += hit;
            let ch = self.input[self.pos];
            if ch == b'"' {
                let slice = &self.input[start..self.pos];
                self.pos += 1;
                return Some(ParsedStr::Borrowed(slice));
            }
            // ch == b'\\' — slow path from here.
            return self.parse_string_bytes_slow(start);
        }
        None
    }

    pub(crate) fn parse_string_bytes_slow(&mut self, start: usize) -> Option<ParsedStr<'a>> {
        let mut result = Vec::from(&self.input[start..self.pos]);
        loop {
            if self.pos >= self.input.len() {
                return None;
            }
            let ch = self.input[self.pos];
            self.pos += 1;
            match ch {
                b'"' => return Some(ParsedStr::Owned(result)),
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

    /// Issue #179 typed-parse fast path. Called when parsing a record
    /// inside a typed-array parse — object shape is known, fields are
    /// expected (but not required) to arrive in declared order.
    #[inline]
    pub(crate) unsafe fn parse_object_shaped(&mut self, shape: &ObjectShapeHint) -> JSValue {
        self.advance(); // past `{`
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();

        // Pre-allocate with the known keys_array + field count. No
        // shape cache lookup — the shape is already in the cache from
        // the one-time build at parse entry.
        let mut js_obj = crate::object::js_object_alloc_class_inline_keys(
            0, // class_id 0 = plain object (not a class instance)
            0, // parent_class_id
            shape.field_count,
            shape.keys_array,
        );
        // Initialize all fields to undefined so JSON with missing
        // fields returns `undefined` for absent properties (matches
        // spec: access to absent own property returns undefined).
        let alloc_field_count =
            std::cmp::max(shape.field_count as usize, crate::object::INLINE_SLOT_FLOOR);
        for i in 0..alloc_field_count {
            let fields_ptr =
                (js_obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut JSValue;
            // GC_STORE_AUDIT(INIT): shaped JSON object fields are initialized before parse publication.
            std::ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        let obj_slot = parse_root_push(JSValue::object_ptr(js_obj as *mut u8));

        // Fast path: track the expected next-field index. Each
        // iteration: if the incoming key matches `expected_keys[idx]`,
        // write to fields[idx] directly and bump. Otherwise fall
        // through to the generic named-setter (which handles
        // out-of-order, extra, or renamed fields).
        let mut fast_idx: usize = 0;
        let field_count = shape.expected_keys.len();

        if self.peek() == Some(b'}') {
            self.advance();
            parse_root_restore(saved_roots);
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
            // Use `parse_value_generic` — nested values inside a
            // shaped record are NOT themselves expected to match the
            // shape (shape is one-level deep by design in Step 1b).
            let value = self.parse_value_generic();
            // JSON.parse suppresses GC for the whole parse, so there is
            // no collection point between `parse_value_generic` and the
            // direct/slow-path field write below.
            js_obj = parse_root_object_ptr(obj_slot);

            let key_bytes = key.as_bytes();

            // Fast path: matches expected next field?
            let mut took_fast = false;
            if fast_idx < field_count {
                let expected = shape.expected_keys[fast_idx];
                if !expected.is_null() {
                    let expected_len = (*expected).byte_len as usize;
                    if expected_len == key_bytes.len() {
                        let expected_data =
                            (expected as *const u8).add(std::mem::size_of::<StringHeader>());
                        let expected_slice =
                            std::slice::from_raw_parts(expected_data, expected_len);
                        if expected_slice == key_bytes {
                            // Match — direct field write.
                            let alloc_limit = alloc_field_count;
                            if fast_idx < alloc_limit {
                                let slot_idx = fast_idx;
                                let value_bits = value.bits();
                                // GC_STORE_AUDIT(BARRIERED): shaped JSON field write uses the shared object slot-store helper.
                                crate::object::store_object_field_slot(
                                    js_obj, slot_idx, value_bits,
                                );
                                fast_idx += 1;
                                took_fast = true;
                            }
                        }
                    }
                }
            }
            if !took_fast {
                // Slow path: might be an out-of-order field, an extra
                // field not in the declared shape, or a shape mismatch.
                // Use the generic named setter which handles all three
                // via transition cache + overflow map. This also
                // pins `fast_idx` — once we slow-path, we stay slow
                // for the rest of the object because the field-index
                // assumption is broken.
                //
                // Key interning: check PARSE_KEY_CACHE first (same
                // path as generic parse_object).
                let key_ptr = cached_parse_key_ptr(key_bytes);
                js_obj = parse_root_object_ptr(obj_slot);
                crate::object::js_object_set_field_by_name(
                    js_obj,
                    key_ptr as *mut StringHeader,
                    f64::from_bits(value.bits()),
                );
                // Force slow path for the rest of this object.
                fast_idx = field_count;
            }

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b'}');
        js_obj = parse_root_object_ptr(obj_slot);
        parse_root_restore(saved_roots);
        JSValue::object_ptr(js_obj as *mut u8)
    }

    /// Issue #179 typed-parse entry: expects `[{…}, {…}, …]` where
    /// each element matches `shape`. Top-level array only; nested
    /// objects inside a record use the generic path.
    #[inline]
    pub(crate) unsafe fn parse_array_typed(&mut self) -> JSValue {
        self.skip_whitespace();
        if self.peek() != Some(b'[') {
            // Shape mismatch — fall through to generic value parse
            // (e.g. Typed<Record> on a `{…}` input still works, just
            // without the array-outer shape).
            return self.parse_value_generic();
        }
        self.advance();
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();
        // Silly-but-effective hot-path guess: large JSON feeds commonly
        // contain arrays of objects (`[{...}, ...]`). Pre-sizing those
        // object arrays avoids repeated grow/copy cycles while keeping
        // scalar/string arrays on the old 16-slot default.
        let mut js_arr = js_array_alloc(if self.peek() == Some(b'{') {
            // 96 B/object is an empirical average for small JSON objects
            // (e.g. `{"id":1,"name":"x"}` ≈ 80-120 B with separators).
            // Clamped to 16..16_384 so tiny payloads stay cheap and
            // multi-MB documents don't over-commit when the average drifts.
            ((self.input.len() - self.pos) / 96).clamp(16, 16_384) as u32
        } else {
            16
        });
        let arr_slot = parse_root_push(JSValue::object_ptr(js_arr as *mut u8));

        if self.peek() == Some(b']') {
            self.advance();
            parse_root_restore(saved_roots);
            return JSValue::object_ptr(js_arr as *mut u8);
        }

        // Take shape pointer once; parse_object_shaped borrows via raw.
        let shape_ptr: *const ObjectShapeHint = self.shape.as_ref().unwrap();

        loop {
            self.skip_whitespace();
            // Per-element: shaped object or generic value (if element
            // isn't an object, fall back).
            let value = if self.peek() == Some(b'{') {
                self.parse_object_shaped(&*shape_ptr)
            } else {
                self.parse_value_generic()
            };
            js_arr = parse_root_array_ptr(arr_slot);
            // GC is suppressed for the whole typed parse, so array growth
            // cannot collect before `value` is stored.
            js_arr = self.array_push_parse_fast(js_arr, value);
            parse_root_set(arr_slot, JSValue::object_ptr(js_arr as *mut u8));

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b']');
        js_arr = parse_root_array_ptr(arr_slot);
        parse_root_restore(saved_roots);
        JSValue::object_ptr(js_arr as *mut u8)
    }

    /// Generic `parse_value` — identical to `parse_value` but without
    /// the shape-specialization dispatch. Called from the typed-parse
    /// path for non-object element values and nested values inside a
    /// shaped record.
    #[inline]
    pub(crate) unsafe fn parse_value_generic(&mut self) -> JSValue {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string_value(),
            Some(b'{') => self.parse_object_untyped(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_true(),
            Some(b'f') => self.parse_false(),
            Some(b'n') => self.parse_null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => JSValue::null(),
        }
    }

    pub(crate) unsafe fn parse_object(&mut self) -> JSValue {
        // The top-level entry `parse_value` routes typed-array parses
        // to `parse_array_typed` directly, so by the time we reach
        // `parse_object` here the only callers are (a) untyped parses
        // and (b) nested objects inside a shaped record — both want
        // generic behavior. Delegate to `parse_object_untyped`.
        self.parse_object_untyped()
    }

    pub(crate) unsafe fn parse_object_untyped(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();

        if self.peek() == Some(b'}') {
            self.advance();
            let keys: [*const StringHeader; 0] = [];
            let keys_arr = self.parse_shape_keys_array_hot(&keys);
            let js_obj = crate::object::js_object_alloc_class_inline_keys(0, 0, 0, keys_arr);
            let fields_ptr =
                (js_obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut JSValue;
            for i in 0..8 {
                // GC_STORE_AUDIT(INIT): empty JSON object fields are initialized before parse publication.
                std::ptr::write(fields_ptr.add(i), JSValue::undefined());
            }
            parse_root_restore(saved_roots);
            return JSValue::object_ptr(js_obj as *mut u8);
        }

        let mut inline_keys: [*const StringHeader; 8] = [std::ptr::null(); 8];
        let mut inline_values: [JSValue; 8] = [JSValue::undefined(); 8];
        let mut inline_len: usize = 0;
        let mut heap_fields: Option<(Vec<*const StringHeader>, Vec<JSValue>)> = None;

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
            // JSON.parse suppresses GC for the whole parse, so key
            // interning cannot collect before `value` is copied into
            // the temporary values vector below.

            let key_bytes = key.as_bytes();
            let key_ptr = cached_parse_key_ptr(key_bytes);
            if let Some((keys, values)) = heap_fields.as_mut() {
                if let Some(existing) = keys.iter().position(|&ptr| ptr == key_ptr) {
                    values[existing] = value;
                } else {
                    keys.push(key_ptr);
                    values.push(value);
                }
            } else if let Some(existing) = inline_keys[..inline_len]
                .iter()
                .position(|&ptr| ptr == key_ptr)
            {
                inline_values[existing] = value;
            } else if inline_len < inline_keys.len() {
                inline_keys[inline_len] = key_ptr;
                inline_values[inline_len] = value;
                inline_len += 1;
            } else {
                let mut keys = Vec::with_capacity(16);
                let mut values = Vec::with_capacity(16);
                keys.extend_from_slice(&inline_keys);
                values.extend_from_slice(&inline_values);
                keys.push(key_ptr);
                values.push(value);
                heap_fields = Some((keys, values));
            }

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b'}');
        let field_count = heap_fields
            .as_ref()
            .map_or(inline_len, |(keys, _)| keys.len()) as u32;
        let keys_arr = if let Some((keys, _)) = heap_fields.as_ref() {
            self.parse_shape_keys_array_hot(keys)
        } else {
            self.parse_shape_keys_array_hot(&inline_keys[..inline_len])
        };
        let js_obj = crate::object::js_object_alloc_class_inline_keys(0, 0, field_count, keys_arr);
        let alloc_field_count =
            std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
        let fields_ptr =
            (js_obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            std::ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        let write_field = |i: usize, value: JSValue| {
            let value_bits = value.bits();
            unsafe {
                // GC_STORE_AUDIT(BARRIERED): JSON object field write uses the shared object slot-store helper.
                crate::object::store_object_field_slot(js_obj, i, value_bits);
            }
        };
        if let Some((_, values)) = heap_fields.as_ref() {
            for (i, value) in values.iter().copied().enumerate() {
                write_field(i, value);
            }
        } else {
            for (i, value) in inline_values[..inline_len].iter().copied().enumerate() {
                write_field(i, value);
            }
        }
        parse_root_restore(saved_roots);
        JSValue::object_ptr(js_obj as *mut u8)
    }

    pub(crate) unsafe fn parse_array(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();
        // Same `[{...}]` pre-size heuristic as the typed path.
        let mut js_arr = js_array_alloc(if self.peek() == Some(b'{') {
            // 96 B/object is an empirical average for small JSON objects
            // (e.g. `{"id":1,"name":"x"}` ≈ 80-120 B with separators).
            // Clamped to 16..16_384 so tiny payloads stay cheap and
            // multi-MB documents don't over-commit when the average drifts.
            ((self.input.len() - self.pos) / 96).clamp(16, 16_384) as u32
        } else {
            16
        });
        let arr_slot = parse_root_push(JSValue::object_ptr(js_arr as *mut u8));

        if self.peek() == Some(b']') {
            self.advance();
            parse_root_restore(saved_roots);
            return JSValue::object_ptr(js_arr as *mut u8);
        }

        loop {
            let value = self.parse_value();
            js_arr = parse_root_array_ptr(arr_slot);
            // GC is suppressed for the whole direct parse, so array growth
            // cannot collect before `value` is stored.
            js_arr = self.array_push_parse_fast(js_arr, value);
            // js_array_push may have returned a new ArrayHeader* after grow;
            // update the root slot so GC sees the new pointer, not the stale one.
            parse_root_set(arr_slot, JSValue::object_ptr(js_arr as *mut u8));

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b']');
        js_arr = parse_root_array_ptr(arr_slot);
        parse_root_restore(saved_roots);
        JSValue::object_ptr(js_arr as *mut u8)
    }

    pub(crate) unsafe fn parse_number(&mut self) -> JSValue {
        let start = self.pos;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.advance();
        }
        let int_start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let int_end = self.pos;

        // Pure-integer fast path: no `.`, no `e`/`E`, value fits in i64.
        // Hot path on JSON payloads with small int fields (record IDs,
        // counters, indices). Skips Rust's general str→f64 parser (which
        // walks the bytes again, runs the Eisel-Lemire / fallback decimal
        // algorithm). Direct accumulator: ~5× faster on a 17-digit
        // input, dominant on real-world id-heavy payloads.
        let has_dot = self.pos < self.input.len() && self.input[self.pos] == b'.';
        let has_exp = self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E');
        if !has_dot && !has_exp {
            let int_len = int_end - int_start;
            if int_len > 0 && int_len <= 18 {
                // 18 digits fits in u64 with room for sign. We've already
                // verified all bytes are ASCII digits, so the cast is safe
                // and the multiply chain doesn't overflow.
                let mut acc: u64 = 0;
                for &b in &self.input[int_start..int_end] {
                    acc = acc * 10 + (b - b'0') as u64;
                }
                let value = if neg { -(acc as f64) } else { acc as f64 };
                return JSValue::number(value);
            }
        }

        // Small fixed-point fast path: JSON API feeds often contain
        // `"score": 123.5`-style values. Avoid the general decimal
        // parser for short non-exponent decimals by accumulating the
        // integer and fractional digits once and scaling by a tiny table.
        if has_dot {
            self.pos += 1;
            let frac_start = self.pos;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            let frac_end = self.pos;
            let exp_after_frac = self.pos < self.input.len()
                && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E');
            let int_len = int_end - int_start;
            let frac_len = frac_end - frac_start;
            if !exp_after_frac && int_len > 0 && int_len <= 15 && frac_len > 0 && frac_len <= 9 {
                let mut int_acc: u64 = 0;
                for &b in &self.input[int_start..int_end] {
                    int_acc = int_acc * 10 + (b - b'0') as u64;
                }
                let mut frac_acc: u64 = 0;
                for &b in &self.input[frac_start..frac_end] {
                    frac_acc = frac_acc * 10 + (b - b'0') as u64;
                }
                const POW10: [f64; 10] = [
                    1.0,
                    10.0,
                    100.0,
                    1_000.0,
                    10_000.0,
                    100_000.0,
                    1_000_000.0,
                    10_000_000.0,
                    100_000_000.0,
                    1_000_000_000.0,
                ];
                let magnitude = int_acc as f64 + (frac_acc as f64 / POW10[frac_len]);
                let value = if neg { -magnitude } else { magnitude };
                return JSValue::number(value);
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

    pub(crate) unsafe fn parse_true(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"true" {
            self.pos += 4;
            JSValue::bool(true)
        } else {
            JSValue::null()
        }
    }

    pub(crate) unsafe fn parse_false(&mut self) -> JSValue {
        if self.pos + 5 <= self.input.len() && &self.input[self.pos..self.pos + 5] == b"false" {
            self.pos += 5;
            JSValue::bool(false)
        } else {
            JSValue::null()
        }
    }

    pub(crate) unsafe fn parse_null(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"null" {
            self.pos += 4;
        }
        JSValue::null()
    }
}
