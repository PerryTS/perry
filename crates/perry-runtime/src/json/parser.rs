//! Direct recursive-descent JSON parser used by `JSON.parse()`.
//!
//! Builds Perry `JSValue`s directly (no intermediate AST). Includes a
//! zero-copy fast path for unescaped string values and an "object-shape
//! hint" specialization used by `js_json_parse_typed_array`.

use super::*;
use crate::{
    array::{note_array_slot_layout_only, ArrayHeader},
    js_array_alloc, js_array_push, JSValue, StringHeader,
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

/// Per-object duplicate-key index for wide JSON objects.
///
/// Property names are untrusted, so `ahash::RandomState` computes a randomly
/// keyed, hash-flood-resistant digest over every incoming byte string. The
/// table stores that result as a `u64`: growth can then rehash the integer in
/// constant time instead of re-reading every managed string. A matching hash
/// is never accepted by itself; exact bytes decide identity, and genuine hash
/// collisions retain their additional indices in `collisions`.
struct ParsedObjectIndex {
    hash_state: ahash::RandomState,
    primary: crate::fast_hash::PtrHashMap<u64, usize>,
    collisions: Vec<(u64, usize)>,
}

impl ParsedObjectIndex {
    unsafe fn from_keys(keys: &[*const StringHeader]) -> Self {
        let mut index = Self {
            hash_state: ahash::RandomState::new(),
            primary: crate::fast_hash::PtrHashMap::with_capacity_and_hasher(
                keys.len(),
                crate::fast_hash::PtrHasher,
            ),
            collisions: Vec::new(),
        };
        for (slot, &key) in keys.iter().enumerate() {
            let bytes = std::slice::from_raw_parts(
                crate::string::string_data(key),
                (*key).byte_len as usize,
            );
            let hash = index.hash_bytes(bytes);
            index.insert_hash(hash, slot);
        }
        index
    }

    #[inline]
    fn hash_bytes(&self, bytes: &[u8]) -> u64 {
        self.hash_state.hash_one(bytes)
    }

    #[inline]
    unsafe fn find_hashed(
        &self,
        hash: u64,
        bytes: &[u8],
        keys: &[*const StringHeader],
    ) -> Option<usize> {
        let first = *self.primary.get(&hash)?;
        if json_key_bytes_equal(keys[first], bytes) {
            return Some(first);
        }
        self.collisions
            .iter()
            .filter(|(candidate_hash, _)| *candidate_hash == hash)
            .map(|(_, slot)| *slot)
            .find(|&slot| json_key_bytes_equal(keys[slot], bytes))
    }

    #[inline]
    fn insert_hash(&mut self, hash: u64, value_index: usize) {
        match self.primary.entry(hash) {
            std::collections::hash_map::Entry::Occupied(_) => {
                self.collisions.push((hash, value_index));
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(value_index);
            }
        }
    }
}

#[inline]
unsafe fn json_key_bytes_equal(key: *const StringHeader, bytes: &[u8]) -> bool {
    (*key).byte_len as usize == bytes.len()
        && std::slice::from_raw_parts(crate::string::string_data(key), bytes.len()) == bytes
}

#[inline]
fn decode_hex_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.len() != 4 {
        return None;
    }
    let mut value = 0u16;
    for &byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        value = (value << 4) | digit as u16;
    }
    Some(value)
}

#[inline]
fn push_code_unit_wtf8(output: &mut Vec<u8>, unit: u16) {
    if (0xD800..=0xDFFF).contains(&unit) {
        output.push(0xE0 | (unit >> 12) as u8);
        output.push(0x80 | ((unit >> 6) & 0x3F) as u8);
        output.push(0x80 | (unit & 0x3F) as u8);
    } else {
        let ch =
            char::from_u32(unit as u32).expect("non-surrogate UTF-16 unit is a Unicode scalar");
        let mut buf = [0u8; 4];
        output.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
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
    /// Pre-built keys array that each parsed record's shape descriptor
    /// references. Built via `js_build_class_keys_array`, so the
    /// shape cache + scan_shape_cache_roots keeps it alive.
    pub(crate) keys_array: *mut crate::array::ArrayHeader,
    /// Number of fields in the declared shape — used as the object's
    /// pre-allocated field count.
    pub(crate) field_count: u32,
}

/// The deepest `[`/`{` nesting handled by the recursive fast path.
///
/// Both the `serde_json` validation pass and Perry's direct value parser recurse
/// once per container, so their cutoff is sized for Perry's smallest worker
/// stack rather than the main thread's larger stack (#7792).
///
/// Deeper documents switch to the flat-tape parser and iterative materializer,
/// so this is a native-stack safety threshold rather than an input limit.
pub(crate) const MAX_RECURSIVE_NESTING_DEPTH: usize = 1_000;

/// Heap-stack safety ceiling. This remains well above Node-parity cases such as
/// #7817's 300,000-level document, while bounding the tape, pending-frame stack,
/// and runtime-container amplification for unusually deep input.
pub(crate) const MAX_ITERATIVE_NESTING_DEPTH: usize = 500_000;

/// Does `bytes` nest deeper than `limit`?
///
/// Iterative on purpose. A recursive depth check would be the very thing it
/// exists to prevent, and it would crash on exactly the documents it is
/// supposed to reject.
///
/// Bracket bytes inside strings do not count, so a document that is one long
/// `"[[[[[[…"` string is not mistaken for deep nesting. This runs before any
/// syntax validation, so it must not assume the input is well-formed — an
/// unbalanced `]` clamps at zero rather than underflowing.
#[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
pub(crate) fn nesting_depth_exceeds(bytes: &[u8], limit: usize) -> bool {
    // If no byte after the root can open a container, the input cannot exceed
    // depth one. This proof remains valid for quoted, escaped and malformed
    // text because a false positive opening only sends us to the full scan;
    // syntax validation remains the parser's job.
    if bytes.len() >= 256 && matches!(bytes[0], b'[' | b'{') && limit > 0 {
        let body = &bytes[1..];
        if !body.contains(&b'{') && !body.contains(&b'[') {
            return false;
        }
    }
    let mut depth = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            b'"' => {
                pos += 1;
                #[cfg(target_arch = "aarch64")]
                let start = pos;
                // A quoted span can be megabytes long. Skip ordinary bytes in
                // bulk while retaining the preflight's handling of malformed
                // input: only quotes/backslashes change string state here.
                while pos < bytes.len() {
                    #[cfg(target_arch = "aarch64")]
                    let offset = depth_string::find_quote_or_backslash(&bytes[pos..]);
                    #[cfg(not(target_arch = "aarch64"))]
                    let offset = super::simd::find_quote_or_backslash(&bytes[pos..]);
                    let Some(offset) = offset else {
                        return false;
                    };
                    pos += offset;
                    if bytes[pos] == b'"' {
                        break;
                    }
                    // Skip the backslash and its escaped byte, including an
                    // escaped quote/backslash. A trailing escape ends the scan;
                    // the real parser remains responsible for syntax errors.
                    pos = (pos + 2).min(bytes.len());
                    // Amortize block classification over longer escaped spans.
                    // Short strings keep the existing quote/escape loop.
                    #[cfg(target_arch = "aarch64")]
                    if pos - start >= 128 {
                        let Some(end) = depth_string::quoted_end(&bytes[pos..]) else {
                            return false;
                        };
                        pos += end;
                        break;
                    }
                }
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        pos += 1;
    }
    false
}

pub(crate) struct DirectParser<'a> {
    input: &'a [u8],
    pos: usize,
    valid: bool,
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
    hot_shape_id: u32,
    /// The newest small parse shape, copied once at the parse boundary. A
    /// top-level record can consume it while reading keys in order, avoiding
    /// one TLS + RefCell key-cache probe per field. GC is already suppressed
    /// before `new_batched`, so these cache-owned pointers cannot move while
    /// the hint is live.
    warm_record_shape_pending: bool,
    /// At least one object crossed into the object-local content index.
    /// Keep the shared key cache stable through recursive parsing, then drop
    /// it at the outer parse boundary so wide schemas cannot pin arena blocks.
    saw_wide_object: bool,
    /// Rooted heap string that owns `input`, or null for standalone parser
    /// tests and non-string sources.
    source: *const StringHeader,
    /// Snapshot the one reusable token once at the parse boundary. A miss then
    /// costs nothing inside record-heavy string loops.
    cached_string: Option<ParseStringReuse>,
    batch: Option<crate::arena::ConstructionBatch>,
}

impl<'a> DirectParser<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            valid: true,
            shape: None,
            hot_shape_len: 0,
            hot_shape_keys: [std::ptr::null(); 8],
            hot_shape_array: std::ptr::null_mut(),
            hot_shape_id: 0,
            warm_record_shape_pending: false,
            saw_wide_object: false,
            source: std::ptr::null(),
            cached_string: None,
            batch: None,
        }
    }

    /// Enter only after the parse API roots its input and suppresses collection.
    pub(crate) unsafe fn new_batched(input: &'a [u8]) -> Self {
        let mut parser = Self::new(input);
        parser.batch = crate::arena::ConstructionBatch::new();
        if (65..=256).contains(&input.len()) && input.first() == Some(&b'{') {
            PARSE_SHAPE_CACHE.with(|cache| {
                let cache = cache.borrow();
                if let Some(entry) = cache.last().filter(|entry| entry.keys.len() <= 8) {
                    parser.hot_shape_len = entry.keys.len();
                    parser.hot_shape_keys[..entry.keys.len()].copy_from_slice(&entry.keys);
                    parser.hot_shape_array = entry.keys_array;
                    parser.hot_shape_id = entry.shape_id;
                    parser.warm_record_shape_pending = true;
                }
            });
        }
        parser
    }

    /// Attach the rooted source identity so immutable string tokens can be
    /// reused across repeated parses of the same JS string.
    pub(crate) unsafe fn new_batched_from_string(
        input: &'a [u8],
        source: *const StringHeader,
    ) -> Self {
        let mut parser = Self::new_batched(input);
        parser.source = source;
        parser.cached_string = cached_parse_string(source, input.len());
        parser
    }

    pub(crate) fn with_shape(input: &'a [u8], shape: ObjectShapeHint) -> Self {
        Self {
            input,
            pos: 0,
            valid: true,
            shape: Some(shape),
            hot_shape_len: 0,
            hot_shape_keys: [std::ptr::null(); 8],
            hot_shape_array: std::ptr::null_mut(),
            hot_shape_id: 0,
            warm_record_shape_pending: false,
            saw_wide_object: false,
            source: std::ptr::null(),
            cached_string: None,
            batch: None,
        }
    }

    #[inline]
    unsafe fn parse_shape_keys_array_hot(
        &mut self,
        keys: &[*const StringHeader],
    ) -> (*mut ArrayHeader, u32) {
        if keys.len() <= self.hot_shape_keys.len()
            && keys.len() == self.hot_shape_len
            && !self.hot_shape_array.is_null()
            && self.hot_shape_keys[..self.hot_shape_len]
                .iter()
                .zip(keys.iter())
                .all(|(a, b)| std::ptr::eq(*a, *b))
        {
            return (self.hot_shape_array, self.hot_shape_id);
        }

        let (keys_array, shape_id) = parse_shape_keys_array_with_id(keys);
        if keys.len() <= self.hot_shape_keys.len() {
            self.hot_shape_len = keys.len();
            self.hot_shape_keys[..keys.len()].copy_from_slice(keys);
            self.hot_shape_array = keys_array;
            self.hot_shape_id = shape_id;
        }
        (keys_array, shape_id)
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

    /// The direct parser constructs values and validates syntax in the same
    /// pass. Call this after the root value to reject both a grammar failure
    /// and a second non-whitespace root token.
    pub(crate) fn finish(&mut self) -> bool {
        self.skip_whitespace();
        if self.saw_wide_object {
            super::parse_scalar::clear_key_cache();
        }
        self.valid && self.pos == self.input.len()
    }

    #[inline]
    pub(crate) fn expect(&mut self, ch: u8) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(ch) {
            self.advance();
            true
        } else {
            self.valid = false;
            false
        }
    }

    #[inline]
    fn invalid_value(&mut self) -> JSValue {
        self.valid = false;
        JSValue::null()
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
            _ => self.invalid_value(),
        }
    }

    pub(crate) unsafe fn parse_string_value(&mut self) -> JSValue {
        let token_start = self.pos;
        if let Some(cached) = self
            .cached_string
            .filter(|cached| cached.token_start == token_start)
        {
            debug_assert!(cached.token_end <= self.input.len());
            self.pos = cached.token_end;
            return JSValue::string_ptr(cached.value as *mut StringHeader);
        }
        if let Some(s) = self.parse_string_bytes() {
            let b = s.as_bytes();
            let value_len = b.len();
            let borrowed = matches!(s, ParsedStr::Borrowed(_));
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
            if b.len() <= crate::value::SHORT_STRING_MAX_LEN
                && !crate::string::bytes_have_lone_surrogate(b)
            {
                let sso = JSValue::short_string_unchecked(b);
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
            let ptr = match s {
                ParsedStr::Borrowed(b) => crate::string::string_from_json_bytes(&mut self.batch, b),
                // Escaped strings live in a Rust Vec, so the builder can derive
                // the WTF-8 lone-surrogate flag while allocating the result.
                ParsedStr::Owned(ref b) => crate::string::js_string_from_builder_bytes(b),
            };
            if borrowed {
                remember_parse_string(
                    self.source,
                    self.input.len(),
                    token_start,
                    self.pos,
                    ptr,
                    value_len,
                );
            }
            JSValue::string_ptr(ptr)
        } else {
            self.invalid_value()
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
    #[inline(never)]
    pub(crate) fn parse_string_bytes(&mut self) -> Option<ParsedStr<'a>> {
        if self.peek() != Some(b'"') {
            self.valid = false;
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
            if ch < 0x20 {
                self.valid = false;
                return None;
            }
            // ch == b'\\' — slow path from here.
            return self.parse_string_bytes_slow(start);
        }
        self.valid = false;
        None
    }

    /// Speculatively consume the next object key in its common, unescaped
    /// spelling while comparing it with the warm shape key. The expected
    /// length tells us exactly where the closing quote must be, so a matching
    /// key needs one short byte walk instead of a terminator scan followed by
    /// a second managed-string comparison. Any escape, control byte, or shape
    /// mismatch restarts at the untouched opening quote through the full JSON
    /// string decoder.
    #[inline(always)]
    unsafe fn parse_string_bytes_expected(
        &mut self,
        expected: *const StringHeader,
    ) -> Option<(ParsedStr<'a>, bool)> {
        if self.peek() == Some(b'"') && !expected.is_null() {
            let start = self.pos + 1;
            let expected_len = (*expected).byte_len as usize;
            if let Some(end) = start.checked_add(expected_len) {
                if end < self.input.len() && self.input[end] == b'"' {
                    let input_bytes = &self.input[start..end];
                    let expected_bytes = std::slice::from_raw_parts(
                        crate::string::string_data(expected),
                        expected_len,
                    );
                    let matches = input_bytes
                        .iter()
                        .zip(expected_bytes)
                        .all(|(&actual, &want)| {
                            actual == want && actual >= 0x20 && actual != b'"' && actual != b'\\'
                        });
                    if matches {
                        self.pos = end + 1;
                        return Some((ParsedStr::Borrowed(input_bytes), true));
                    }
                }
            }
        }

        self.parse_string_bytes().map(|key| (key, false))
    }

    #[inline(never)]
    pub(crate) fn parse_string_bytes_slow(&mut self, start: usize) -> Option<ParsedStr<'a>> {
        let mut result = Vec::from(&self.input[start..self.pos]);
        // Keep short strings and allocation growth on the scalar path. Chunk
        // decoding only consumes existing spare capacity; it never grows the
        // scratch buffer early just to satisfy a worst-case output bound.
        let mut scalar_end = self.input.len().min(self.pos.saturating_add(64));
        loop {
            if self.pos >= scalar_end {
                while self.input.len() - self.pos >= 64 && result.capacity() - result.len() >= 64 {
                    if self.decode_chunk(&mut result)? {
                        return Some(ParsedStr::Owned(result));
                    }
                }
                scalar_end = self.input.len().min(self.pos.saturating_add(64));
                if self.pos >= self.input.len() {
                    self.valid = false;
                    return None;
                }
            }
            // scalar_end never exceeds input.len(), including after chunk decoding.
            let ch = unsafe { *self.input.get_unchecked(self.pos) };
            self.pos += 1;
            match ch {
                b'"' => return Some(ParsedStr::Owned(result)),
                b'\\' => {
                    if self.pos >= self.input.len() {
                        self.valid = false;
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
                                self.valid = false;
                                return None;
                            }
                            let Some(code) = decode_hex_u16(&self.input[self.pos..self.pos + 4])
                            else {
                                self.valid = false;
                                return None;
                            };
                            self.pos += 4;
                            let paired_low = if (0xD800..=0xDBFF).contains(&code)
                                && self.pos + 6 <= self.input.len()
                                && self.input[self.pos] == b'\\'
                                && self.input[self.pos + 1] == b'u'
                            {
                                decode_hex_u16(&self.input[self.pos + 2..self.pos + 6])
                                    .filter(|low| (0xDC00..=0xDFFF).contains(low))
                            } else {
                                None
                            };
                            if let Some(low) = paired_low {
                                self.pos += 6;
                                let codepoint = 0x10000
                                    + ((code as u32 - 0xD800) << 10)
                                    + (low as u32 - 0xDC00);
                                let c = char::from_u32(codepoint)
                                    .expect("surrogate pair is a Unicode scalar");
                                let mut buf = [0u8; 4];
                                let s = c.encode_utf8(&mut buf);
                                result.extend_from_slice(s.as_bytes());
                            } else {
                                push_code_unit_wtf8(&mut result, code);
                            }
                        }
                        _ => {
                            self.valid = false;
                            return None;
                        }
                    }
                }
                c if c < 0x20 => {
                    self.valid = false;
                    return None;
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
        let mut saw_pointer = false;
        let mut js_obj = crate::object::js_object_alloc_class_inline_keys(
            0, // class_id 0 = plain object (not a class instance)
            0, // parent_class_id
            shape.field_count,
            shape.keys_array,
        );
        // #8098: parsed records are ordinary plain objects — no class, but an
        // authoritative ShapeId and no per-object [[Set]] semantics — so mark
        // them eligible for the object-write fast paths.
        crate::object::mark_object_plain_ordinary(js_obj);
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
            if !self.valid {
                break;
            }
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
                                // GC_STORE_AUDIT(BARRIERED): shaped JSON field write uses the
                                // layout-deferred slot-store helper (#7630); the layout state
                                // is settled once at the tail of this function.
                                saw_pointer |=
                                    crate::object::store_object_field_slot_layout_deferred(
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
                // The by-name path stores through the noting helper and may
                // build a mask mid-construction; treat it as pointer-bearing so
                // the tail's finalize (which routes through layout_mark_unknown)
                // removes whatever it recorded (#7630).
                saw_pointer = true;
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
        // #7630: the construction loop elided per-slot layout notes; settle the
        // layout state once, on the LIVE pointer (re-read from the parse root
        // above, so a mid-parse collection cannot leave this on a stale copy).
        crate::gc::layout_finish_deferred_boxed_object(js_obj as usize, saw_pointer);
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
            if !self.valid {
                break;
            }
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
            _ => self.invalid_value(),
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

        // The parse-boundary hint seeds the first root record. After any
        // object establishes a small shape, let the next object speculate on
        // that parser-local shape too. Homogeneous record arrays then compare
        // key bytes directly against six cached pointers instead of entering
        // the TLS key cache for every field of every record. Any mismatch
        // drops back to the content-keyed path below.
        let warm_shape = if self.warm_record_shape_pending {
            self.warm_record_shape_pending = false;
            Some((
                self.hot_shape_len,
                self.hot_shape_keys,
                self.hot_shape_array,
                self.hot_shape_id,
            ))
        } else if self.hot_shape_len != 0 && !self.hot_shape_array.is_null() {
            Some((
                self.hot_shape_len,
                self.hot_shape_keys,
                self.hot_shape_array,
                self.hot_shape_id,
            ))
        } else {
            None
        };
        let mut warm_shape_slot = 0usize;
        let mut warm_shape_matches = warm_shape.is_some();

        let saved_roots = parse_root_save_len();

        if self.peek() == Some(b'}') {
            self.advance();
            let keys: [*const StringHeader; 0] = [];
            let (keys_arr, shape_id) = self.parse_shape_keys_array_hot(&keys);
            let js_obj = crate::object::object_from_json_fields_preinstalled(
                &mut self.batch,
                keys_arr,
                shape_id,
                &[],
            );
            parse_root_restore(saved_roots);
            return JSValue::object_ptr(js_obj as *mut u8);
        }

        let mut inline_keys: [*const StringHeader; 8] = [std::ptr::null(); 8];
        let mut inline_values: [JSValue; 8] = [JSValue::undefined(); 8];
        let mut inline_len: usize = 0;
        // Keep small-object searches linear, including modest spills past
        // the eight inline slots. Wide objects index interned key identities
        // so new keys do not scan every prior key. The vectors retain order;
        // a duplicate only replaces its value. GC is suppressed for the parse,
        // exactly as for the raw key pointers already held in these vectors.
        type HeapFields = (
            Vec<*const StringHeader>,
            Vec<JSValue>,
            Option<ParsedObjectIndex>,
        );
        let mut heap_fields: Option<HeapFields> = None;

        loop {
            self.skip_whitespace();
            let expected_key = warm_shape.as_ref().and_then(|(len, keys, _, _)| {
                if warm_shape_matches && warm_shape_slot < *len {
                    Some(keys[warm_shape_slot])
                } else {
                    None
                }
            });
            let (key, matched_expected_spelling) = match expected_key {
                Some(expected) => match self.parse_string_bytes_expected(expected) {
                    Some(key) => key,
                    None => break,
                },
                None => match self.parse_string_bytes() {
                    Some(key) => (key, false),
                    None => break,
                },
            };

            if !self.expect(b':') {
                break;
            }

            let value = self.parse_value();
            if !self.valid {
                break;
            }
            // JSON.parse suppresses GC for the whole parse, so key
            // interning cannot collect before `value` is copied into
            // the temporary values vector below.

            let key_bytes = key.as_bytes();
            if let Some((keys, values, indices)) = heap_fields.as_mut() {
                // Linear lookup wins for modest objects. Build the index only
                // when another field arrives after 128 unique keys, so an
                // object ending at that size never pays to build an unused map.
                if indices.is_none() && keys.len() == 128 {
                    // From here this object's content index owns duplicate
                    // detection. Defer clearing the shared cache until finish:
                    // a nested wide object must not change key identity while
                    // its enclosing object is still recognizing duplicates.
                    self.saw_wide_object = true;
                    *indices = Some(ParsedObjectIndex::from_keys(keys));
                }
                if let Some(index) = indices {
                    let hash = index.hash_bytes(key_bytes);
                    if let Some(existing) = index.find_hashed(hash, key_bytes, keys) {
                        values[existing] = value;
                    } else {
                        // The object-local content index already proves this
                        // key is new. Avoid duplicating every wide key in the
                        // global interning table only to clear it at return.
                        let key_ptr =
                            crate::string::string_from_json_bytes(&mut self.batch, key_bytes);
                        index.insert_hash(hash, keys.len());
                        keys.push(key_ptr);
                        values.push(value);
                    }
                } else {
                    let key_ptr = cached_parse_key_ptr(key_bytes);
                    let warm_prefix_uses_old_key_identity =
                        warm_shape_slot != 0 && !warm_shape_matches;
                    if let Some(existing) = keys.iter().position(|&ptr| {
                        ptr == key_ptr
                            || (warm_prefix_uses_old_key_identity
                                && json_key_bytes_equal(ptr, key_bytes))
                    }) {
                        values[existing] = value;
                    } else {
                        keys.push(key_ptr);
                        values.push(value);
                    }
                }
            } else {
                let key_ptr = if warm_shape_matches {
                    let (expected_len, expected_keys, _, _) = warm_shape.as_ref().unwrap();
                    if warm_shape_slot < *expected_len
                        && (matched_expected_spelling
                            || json_key_bytes_equal(expected_keys[warm_shape_slot], key_bytes))
                    {
                        let ptr = expected_keys[warm_shape_slot];
                        warm_shape_slot += 1;
                        ptr
                    } else {
                        warm_shape_matches = false;
                        cached_parse_key_ptr(key_bytes)
                    }
                } else {
                    cached_parse_key_ptr(key_bytes)
                };
                let warm_prefix_uses_old_key_identity = warm_shape_slot != 0 && !warm_shape_matches;
                if let Some(existing) = inline_keys[..inline_len].iter().position(|&ptr| {
                    ptr == key_ptr
                        || (warm_prefix_uses_old_key_identity
                            && json_key_bytes_equal(ptr, key_bytes))
                }) {
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
                    heap_fields = Some((keys, values, None));
                }
            }

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b'}');
        let (keys_arr, shape_id) = if let Some((keys, _, _)) = heap_fields.as_ref() {
            self.parse_shape_keys_array_hot(keys)
        } else if warm_shape_matches
            && warm_shape
                .as_ref()
                .is_some_and(|(len, _, _, _)| *len == inline_len && warm_shape_slot == *len)
        {
            let (_, _, keys_array, shape_id) = warm_shape.unwrap();
            (keys_array, shape_id)
        } else {
            self.parse_shape_keys_array_hot(&inline_keys[..inline_len])
        };
        let values = heap_fields
            .as_ref()
            .map_or(&inline_values[..inline_len], |(_, values, _)| {
                values.as_slice()
            });
        let js_obj = crate::object::object_from_json_fields_preinstalled(
            &mut self.batch,
            keys_arr,
            shape_id,
            values,
        );
        parse_root_restore(saved_roots);
        JSValue::object_ptr(js_obj as *mut u8)
    }

    pub(crate) unsafe fn parse_array(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();
        if self.peek() != Some(b'{') {
            return self.parse_array_prefix(saved_roots);
        }
        // Same `[{...}]` pre-size heuristic as the typed path.
        // Preserve the object-leading estimate on large record arrays.
        let array = super::construction_array::ConstructionArray::new(
            &mut self.batch,
            ((self.input.len() - self.pos) / 96).clamp(16, 16_384) as u32,
        );
        self.parse_array_tail(array, saved_roots)
    }

    /// The direct parser's existing suppression window protects these native
    /// value slots, just as it protects parse_object_untyped's inline fields.
    /// Child allocations belong to the result graph. Delay the array itself
    /// until its width is known, avoiding sixteen slots for a two-item array.
    #[inline(never)]
    unsafe fn parse_array_prefix(&mut self, saved_roots: usize) -> JSValue {
        let mut values = [JSValue::undefined(); 8];
        let mut used = 0;
        if self.peek() == Some(b']') {
            self.advance();
            return self.finish_short_array(&values[..used], saved_roots);
        }
        loop {
            if used == values.len() {
                // The comma after element eight was consumed. Continue at
                // the ninth value without reparsing any prefix or child.
                let mut array =
                    super::construction_array::ConstructionArray::new(&mut self.batch, 16);
                for &value in &values {
                    array.push(&mut self.batch, value);
                }
                return self.parse_array_tail(array, saved_roots);
            }
            let value = self.parse_value();
            if !self.valid {
                self.expect(b']');
                return self.finish_short_array(&values[..used], saved_roots);
            }
            values[used] = value;
            used += 1;
            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                self.expect(b']');
                return self.finish_short_array(&values[..used], saved_roots);
            }
        }
    }

    unsafe fn finish_short_array(&mut self, values: &[JSValue], saved_roots: usize) -> JSValue {
        let mut array =
            super::construction_array::ConstructionArray::new(&mut self.batch, values.len() as u32);
        for &value in values {
            array.push(&mut self.batch, value);
        }
        let result = array.finish(&self.batch);
        parse_root_restore(saved_roots);
        JSValue::object_ptr(result.cast())
    }

    /// Containers stay private until complete. Collection remains suppressed
    /// for the whole parse; the native builder carries only final output slots
    /// and bounded aggregate layout facts, not a second representation.
    unsafe fn parse_array_tail(
        &mut self,
        mut array: super::construction_array::ConstructionArray,
        saved_roots: usize,
    ) -> JSValue {
        loop {
            let value = self.parse_value();
            if !self.valid {
                break;
            }
            array.push(&mut self.batch, value);
            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b']');
        let result = array.finish(&self.batch);
        parse_root_restore(saved_roots);
        JSValue::object_ptr(result.cast())
    }

    pub(crate) unsafe fn parse_number(&mut self) -> JSValue {
        let start = self.pos;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.advance();
        }
        let int_start = self.pos;
        match self.peek() {
            Some(b'0') => {
                self.advance();
                if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                        self.advance();
                    }
                    return self.invalid_value();
                }
            }
            Some(b'1'..=b'9') => {
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.advance();
                }
            }
            _ => return self.invalid_value(),
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
        // parser for short non-exponent decimals by accumulating ALL
        // the digits into one integer mantissa and dividing once by an
        // exact power of ten.
        //
        // #7477: this must be bit-identical to `str::parse::<f64>` (the
        // tape materializer's and V8-strtod's answer). The previous form
        // `int as f64 + (frac as f64 / 10^k)` rounded TWICE — the
        // division rounds, then the addition rounds again — and was one
        // ulp off for literals like `260.75197`. The single-division
        // form is the classic Clinger fast path: when the decimal
        // mantissa m fits in 2^53 (exactly representable) and 10^k is an
        // exact f64 (all powers up to 10^22 are), `m as f64 / 10^k` is
        // ONE correctly-rounded IEEE operation on the exact rational
        // m/10^k — the same double a correct decimal parser produces.
        // Anything wider falls through to `str::parse` below.
        if has_dot {
            self.pos += 1;
            let frac_start = self.pos;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            let frac_end = self.pos;
            let exp_after_frac = self.pos < self.input.len()
                && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E');
            if frac_end == frac_start {
                return self.invalid_value();
            }
            let int_len = int_end - int_start;
            let frac_len = frac_end - frac_start;
            if !exp_after_frac
                && int_len > 0
                && frac_len > 0
                && frac_len <= 9
                && int_len + frac_len <= 17
            {
                // ≤ 17 digits always fits u64 (10^17 < 2^63); the ≤ 2^53
                // check below is the exact-representability gate.
                let mut mantissa: u64 = 0;
                for &b in &self.input[int_start..int_end] {
                    mantissa = mantissa * 10 + (b - b'0') as u64;
                }
                for &b in &self.input[frac_start..frac_end] {
                    mantissa = mantissa * 10 + (b - b'0') as u64;
                }
                if mantissa <= (1u64 << 53) {
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
                    let magnitude = mantissa as f64 / POW10[frac_len];
                    let value = if neg { -magnitude } else { magnitude };
                    return JSValue::number(value);
                }
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
            let exponent_start = self.pos;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.pos == exponent_start {
                return self.invalid_value();
            }
        }

        let num_str = std::str::from_utf8_unchecked(&self.input[start..self.pos]);
        match num_str.parse::<f64>() {
            Ok(value) => JSValue::number(value),
            Err(_) => self.invalid_value(),
        }
    }

    pub(crate) unsafe fn parse_true(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"true" {
            self.pos += 4;
            JSValue::bool(true)
        } else {
            self.invalid_value()
        }
    }

    pub(crate) unsafe fn parse_false(&mut self) -> JSValue {
        if self.pos + 5 <= self.input.len() && &self.input[self.pos..self.pos + 5] == b"false" {
            self.pos += 5;
            JSValue::bool(false)
        } else {
            self.invalid_value()
        }
    }

    pub(crate) unsafe fn parse_null(&mut self) -> JSValue {
        if self.pos + 4 <= self.input.len() && &self.input[self.pos..self.pos + 4] == b"null" {
            self.pos += 4;
            JSValue::null()
        } else {
            self.invalid_value()
        }
    }
}

#[cfg(test)]
#[path = "parser_scan_tests.rs"]
mod scan_tests;

#[cfg(test)]
#[path = "parser_short_array_tests.rs"]
mod short_array_tests;

#[path = "parser_escape_chunk.rs"]
mod escape_chunk;

#[cfg(test)]
#[path = "parser_escape_chunk_tests.rs"]
mod escape_chunk_tests;

#[cfg(target_arch = "aarch64")]
#[path = "parser_depth_string.rs"]
mod depth_string;

#[cfg(all(test, target_arch = "aarch64"))]
#[path = "parser_depth_string_tests.rs"]
mod depth_string_tests;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[path = "parser_depth_blocks.rs"]
mod depth_blocks;

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
pub(crate) use depth_blocks::nesting_depth_exceeds;

#[cfg(all(test, target_arch = "aarch64", target_endian = "little"))]
#[path = "parser_depth_blocks_tests.rs"]
mod depth_blocks_tests;
