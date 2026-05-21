//! JSON handling — JSON.parse(), JSON.stringify(), and specialized variants
//!
//! Provides all core JSON functions used by compiled TypeScript programs.
//! These live in perry-runtime (not perry-stdlib) so that programs that
//! only use JSON don't need to link the full stdlib.

use crate::string::js_string_from_ascii_bytes;
use crate::{js_array_alloc, js_array_push, js_string_from_bytes, JSValue, StringHeader};
use std::cell::RefCell;
use std::fmt::Write as FmtWrite;

// ─── Circular reference detection ────────────────────────────────────────────
thread_local! {
    /// Stack of object pointers currently being stringified (for circular detection).
    static STRINGIFY_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };

    /// Reusable scratch buffer for JSON.stringify (issue #64). Avoids the
    /// per-call `String::with_capacity` allocate+free that dominated the
    /// small-stringify path. Wrapped in `Cell<Option<_>>` so reentrant calls
    /// (via `toJSON` callbacks etc.) get a fresh buffer instead of panicking
    /// on a `RefCell` borrow conflict; the larger of the two is restored.
    static STRINGIFY_BUF: std::cell::Cell<Option<String>> =
        std::cell::Cell::new(Some(String::with_capacity(4096)));

    /// Per-call shape-template cache (#64 follow-up). Keys on `keys_array`
    /// raw pointer — within one top-level stringify call no GC runs over
    /// the user object graph (the buffer/result allocations don't move
    /// keys arrays), so pointer identity is a stable shape ID. Cleared
    /// (saved+restored) at the entry of each `js_json_stringify` /
    /// `js_json_stringify_full` / `..with_replacer` call so reentrant
    /// `toJSON` callbacks don't return stale templates.
    ///
    /// `Box<ShapeTemplate>` lives on the heap so its address is stable
    /// even when the cache `Vec` reallocates — we hand out raw pointers
    /// to the templates and they must outlive the borrow.
    static SHAPE_CACHE: RefCell<Vec<(*mut crate::ArrayHeader, Box<ShapeTemplate>)>> =
        const { RefCell::new(Vec::new()) };

    /// Key string intern cache for JSON.parse (issue #51 follow-up).
    /// Maps key bytes → already-allocated StringHeader pointer.
    /// Avoids re-allocating "id", "name", etc. for every record in a
    /// homogeneous JSON array. Cleared at the end of each top-level parse.
    /// `pub(crate)` so `json_tape`'s materializer can share the cache —
    /// without this, each tape-path force-materialize re-allocates every
    /// key and burns 3× the time + RSS vs the direct parser.
    pub(crate) static PARSE_KEY_CACHE: RefCell<std::collections::HashMap<Vec<u8>, *const StringHeader>> =
        RefCell::new(std::collections::HashMap::new());

    /// Shared keys_array cache for direct JSON.parse objects.
    ///
    /// The generic object setter builds a shape one property at a time. For
    /// homogeneous JSON records that means every parsed object participates in
    /// the transition-cache machinery before field values are written. The tape
    /// path avoids that churn by materializing from a compact token stream; the
    /// direct parser needs the same shape sharing so `PERRY_JSON_TAPE=0` does
    /// not manufacture short-lived keys arrays and forwarding stubs in old
    /// pages.
    static PARSE_SHAPE_CACHE: RefCell<Vec<ParseShapeCacheEntry>> =
        const { RefCell::new(Vec::new()) };

    /// Reentrancy depth counter for JSON.stringify (issue #67). 0 means
    /// no call in progress; ≥1 means a reentrant (toJSON callback) path.
    /// Used to skip the shape_cache save/restore dance for the common
    /// non-reentrant case — a plain `clear_shape_cache` at the outermost
    /// call's exit handles correctness without a Vec alloc/swap.
    static STRINGIFY_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };

    /// GC roots for in-progress JSON.parse. Each entry is a JSValue bit pattern
    /// (stored as f64 so the scanner can hand it to the NaN-boxed mark path).
    ///
    /// Why this exists (issue #46): parse_array/parse_object build their result
    /// incrementally over thousands of iterations. Mid-parse heap allocations
    /// (`js_string_from_bytes` → gc_malloc → adaptive count trigger, or an arena
    /// block overflow) run GC while the in-progress array/object lives only on
    /// the Rust call stack. The conservative stack scan only captures callee-
    /// saved registers via setjmp; values held in caller-saved regs (or on
    /// the Rust-heap backing of `Vec<(Vec<u8>, JSValue)>` inside parse_object)
    /// are invisible and get swept. Symptom was `JSON.parse(big_array)` silently
    /// truncating at ~1666 records (= when the second adaptive malloc GC fires).
    static PARSE_ROOTS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

struct ParseShapeCacheEntry {
    keys: Vec<*const StringHeader>,
    keys_array: *mut crate::ArrayHeader,
}

const PARSE_SHAPE_CACHE_CAP: usize = 256;

#[inline]
fn parse_root_push(v: JSValue) -> usize {
    PARSE_ROOTS.with(|r| {
        let mut r = r.borrow_mut();
        let idx = r.len();
        r.push(f64::from_bits(v.bits()));
        idx
    })
}

#[inline]
fn parse_root_set(idx: usize, v: JSValue) {
    PARSE_ROOTS.with(|r| {
        if let Some(slot) = r.borrow_mut().get_mut(idx) {
            *slot = f64::from_bits(v.bits());
        }
    });
}

#[inline]
fn parse_root_get(idx: usize) -> JSValue {
    PARSE_ROOTS.with(|r| {
        r.borrow()
            .get(idx)
            .map(|slot| JSValue::from_bits(slot.to_bits()))
            .unwrap_or_else(JSValue::undefined)
    })
}

#[inline]
fn parse_root_object_ptr(idx: usize) -> *mut crate::ObjectHeader {
    (parse_root_get(idx).bits() & POINTER_MASK) as *mut crate::ObjectHeader
}

#[inline]
fn parse_root_array_ptr(idx: usize) -> *mut crate::ArrayHeader {
    (parse_root_get(idx).bits() & POINTER_MASK) as *mut crate::ArrayHeader
}

#[inline]
fn parse_root_save_len() -> usize {
    PARSE_ROOTS.with(|r| r.borrow().len())
}

#[inline]
fn parse_root_restore(len: usize) {
    PARSE_ROOTS.with(|r| r.borrow_mut().truncate(len));
}

#[inline]
fn cached_parse_key_ptr(key_bytes: &[u8]) -> *const StringHeader {
    let cached = PARSE_KEY_CACHE.with(|c| c.borrow().get(key_bytes).copied());
    if let Some(ptr) = cached {
        return ptr;
    }

    // Issue #179: allocate cached key strings in the longlived arena. They
    // are rooted by `scan_parse_roots_mut` and reused across repeated parses
    // of homogeneous JSON records.
    let ptr =
        crate::string::js_string_from_bytes_longlived(key_bytes.as_ptr(), key_bytes.len() as u32);
    PARSE_KEY_CACHE.with(|c| {
        c.borrow_mut().insert(key_bytes.to_vec(), ptr);
    });
    ptr
}

#[inline]
unsafe fn parse_shape_keys_array(keys: &[*const StringHeader]) -> *mut crate::ArrayHeader {
    PARSE_SHAPE_CACHE.with(|cache| {
        {
            let cache = cache.borrow();
            for entry in cache.iter().rev() {
                if entry.keys.len() == keys.len()
                    && entry
                        .keys
                        .iter()
                        .zip(keys.iter())
                        .all(|(a, b)| std::ptr::eq(*a, *b))
                {
                    return entry.keys_array;
                }
            }
        }

        let arr = crate::array::js_array_alloc_with_length_longlived(keys.len() as u32);
        let elements_ptr =
            (arr as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
        for (i, &key_ptr) in keys.iter().enumerate() {
            let bits = crate::value::STRING_TAG | (key_ptr as u64 & crate::value::POINTER_MASK);
            *elements_ptr.add(i) = f64::from_bits(bits);
            crate::gc::layout_note_slot(arr as usize, i, bits);
        }
        let header = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;

        let mut cache = cache.borrow_mut();
        if cache.len() < PARSE_SHAPE_CACHE_CAP {
            cache.push(ParseShapeCacheEntry {
                keys: keys.to_vec(),
                keys_array: arr,
            });
        }
        arr
    })
}

/// Take the shared scratch buffer (or allocate a fresh one on reentrancy).
#[inline]
fn take_stringify_buf() -> String {
    STRINGIFY_BUF.with(|b| b.take()).unwrap_or_default()
}

/// Restore the scratch buffer after use. In a reentrant call the inner
/// restore runs first with a (typically smaller) buffer, but the outer
/// restore runs last and overwrites with its larger buffer — so the
/// final TLS state always holds the largest recently-used buffer without
/// needing a per-call capacity comparison.
#[inline]
fn restore_stringify_buf(mut buf: String) {
    buf.clear();
    STRINGIFY_BUF.with(|b| b.set(Some(buf)));
}

#[inline]
fn json_string_from_output_bytes(bytes: &[u8]) -> *mut StringHeader {
    if bytes.is_ascii() {
        js_string_from_ascii_bytes(bytes.as_ptr(), bytes.len() as u32)
    } else {
        js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }
}

/// Save & clear the shape cache for the duration of a top-level stringify
/// call. Reentrant `toJSON` callbacks would otherwise inherit the outer
/// call's templates and (worse) clear them on exit, dangling pointers we
/// already handed out. Mirrors `take_stringify_buf` in spirit.
#[inline]
fn take_shape_cache() -> Vec<(*mut crate::ArrayHeader, Box<ShapeTemplate>)> {
    SHAPE_CACHE.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

#[inline]
fn restore_shape_cache(saved: Vec<(*mut crate::ArrayHeader, Box<ShapeTemplate>)>) {
    SHAPE_CACHE.with(|c| *c.borrow_mut() = saved);
}

/// Clear cache without allocating a fresh Vec (keeps capacity, drops entries).
/// Used in place of restore when we know the cache was empty at call entry —
/// the outermost stringify call in a tight loop skips the save entirely.
#[inline]
fn clear_shape_cache() {
    SHAPE_CACHE.with(|c| c.borrow_mut().clear());
}

/// Maximum cache entries per call. Workloads with more distinct shapes
/// fall back to per-object building. We never evict — the raw pointers we
/// hand out must outlive cache mutations, and `swap_remove` would drop a
/// `Box` whose heap address might be in active use up the call stack.
/// 32 entries × ~40 bytes ≈ 1.3 KB; a stringify graph that exceeds this
/// gets the pre-cache slow path on overflow shapes only.
const SHAPE_CACHE_CAP: usize = 32;

/// Look up (or build & insert) the shape template for an object. Returns
/// `None` if the object isn't templatable (no keys array, too many fields,
/// malformed key strings) or if the cache is full and missed.
///
/// Returns a raw pointer because lifetimes can't survive the TLS borrow.
/// The pointer stays valid until the next `take_shape_cache` (top-level
/// entry/exit) — within one stringify traversal we only `push`, and
/// `Box`'s heap address is stable across `Vec` growth.
#[inline]
unsafe fn shape_template_for(obj_ptr: *const u8) -> Option<*const ShapeTemplate> {
    let obj = obj_ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }

    SHAPE_CACHE.with(|c| {
        // Fast path: linear scan from the back — recently-used entries
        // cluster there for typical traversal orders (shape A's elements
        // recurse into shape B repeatedly).
        {
            let cache = c.borrow();
            for entry in cache.iter().rev() {
                if entry.0 == keys_arr {
                    return Some(&*entry.1 as *const ShapeTemplate);
                }
            }
            if cache.len() >= SHAPE_CACHE_CAP {
                return None;
            }
        }

        // Miss — build, insert, return raw pointer to the boxed template.
        let elem_bits = make_pointer_bits(obj_ptr);
        let template = build_shape_prefix_template(elem_bits)?;
        let mut cache = c.borrow_mut();
        // Re-check cap after the borrow round-trip (a recursive call
        // during template build could have filled the cache).
        if cache.len() >= SHAPE_CACHE_CAP {
            return None;
        }
        cache.push((keys_arr, Box::new(template)));
        Some(&*cache.last().unwrap().1 as *const ShapeTemplate)
    })
}

#[inline]
fn make_pointer_bits(ptr: *const u8) -> u64 {
    POINTER_TAG | (ptr as u64 & POINTER_MASK)
}

/// Root scanner called by GC — marks every value in PARSE_ROOTS as live.
pub fn scan_parse_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_parse_roots_mut(&mut visitor);
}

pub fn scan_parse_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PARSE_ROOTS.with(|r| {
        for v in r.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(v);
        }
    });
    // Also mark interned key strings so GC doesn't sweep them mid-parse.
    PARSE_KEY_CACHE.with(|c| {
        for ptr in c.borrow_mut().values_mut() {
            visitor.visit_tagged_raw_const_ptr_slot(ptr, crate::value::STRING_TAG);
        }
    });
    PARSE_SHAPE_CACHE.with(|c| {
        for entry in c.borrow_mut().iter_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut entry.keys_array);
            for key in entry.keys.iter_mut() {
                visitor.visit_tagged_raw_const_ptr_slot(key, crate::value::STRING_TAG);
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_parse_roots(value: f64, key_ptr: *const StringHeader) {
    PARSE_ROOTS.with(|r| {
        let mut r = r.borrow_mut();
        r.clear();
        r.push(value);
    });
    PARSE_KEY_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.clear();
        c.insert(b"test".to_vec(), key_ptr);
    });
}

#[cfg(test)]
pub(crate) fn test_parse_roots_snapshot() -> (u64, usize) {
    let value_bits =
        PARSE_ROOTS.with(|r| r.borrow().first().copied().map(f64::to_bits).unwrap_or(0));
    let key_ptr = PARSE_KEY_CACHE.with(|c| {
        c.borrow()
            .get(b"test".as_slice())
            .copied()
            .map(|ptr| ptr as usize)
            .unwrap_or(0)
    });
    (value_bits, key_ptr)
}

// ─── Zero-copy string access ──────────────────────────────────────────────────

#[inline]
unsafe fn str_from_header<'a>(ptr: *const StringHeader) -> Option<&'a str> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(std::str::from_utf8_unchecked(bytes))
}

// ─── SIMD string-terminator scan ──────────────────────────────────────────────

/// Find the offset of the first `"` or `\` in `bytes`. Returns `None`
/// if neither is found before end-of-input (which is a JSON error — the
/// caller handles that by failing the parse).
///
/// Issue #179 tier 1 #3: SIMD-accelerated on aarch64 (NEON) and x86_64
/// (SSE2); scalar on other targets. The hot path on
/// `bench_json_roundtrip` — per-record string scanning — previously
/// ran one byte at a time in the tight zero-copy fast-path loop. 16-byte
/// SIMD chunks cut the per-iteration overhead substantially on long
/// records, and the scalar tail handles the trailing <16 bytes.
#[inline(always)]
fn find_string_terminator(bytes: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        find_string_terminator_neon(bytes)
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        return find_string_terminator_sse2(bytes);
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse2")
    )))]
    {
        find_string_terminator_scalar(bytes)
    }
}

/// Scalar fallback used on non-SIMD targets and as the tail handler
/// for the SIMD variants. Always inlined so the caller's tight loop
/// doesn't pay a call-site cost for the <16-byte tail.
#[inline(always)]
fn find_string_terminator_scalar(bytes: &[u8]) -> Option<usize> {
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' || b == b'\\' {
            return Some(i);
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn find_string_terminator_neon(bytes: &[u8]) -> Option<usize> {
    use std::arch::aarch64::*;
    unsafe {
        let quote = vdupq_n_u8(b'"');
        let bslash = vdupq_n_u8(b'\\');
        let mut i: usize = 0;
        while i + 16 <= bytes.len() {
            let chunk = vld1q_u8(bytes.as_ptr().add(i));
            let eq_q = vceqq_u8(chunk, quote);
            let eq_b = vceqq_u8(chunk, bslash);
            let mask = vorrq_u8(eq_q, eq_b);
            // Fast rejection: reduce the 16-byte mask to a single byte
            // (max across all lanes). Zero => no match in this chunk.
            if vmaxvq_u8(mask) == 0 {
                i += 16;
                continue;
            }
            // Hit somewhere in this chunk — scan the 16 bytes to find
            // the exact offset. Branchless via per-lane comparison.
            // `mask` has 0xFF at matching lane positions and 0x00
            // elsewhere; store-and-scan is portable and fast enough
            // for a 16-byte region.
            let mut lanes = [0u8; 16];
            vst1q_u8(lanes.as_mut_ptr(), mask);
            for (j, &lane) in lanes.iter().enumerate() {
                if lane != 0 {
                    return Some(i + j);
                }
            }
            // Unreachable — vmaxvq_u8 said there's a match.
            unreachable!();
        }
        // Tail: <16 bytes left, scalar scan.
        find_string_terminator_scalar(&bytes[i..]).map(|off| i + off)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
#[inline(always)]
fn find_string_terminator_sse2(bytes: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    unsafe {
        let quote = _mm_set1_epi8(b'"' as i8);
        let bslash = _mm_set1_epi8(b'\\' as i8);
        let mut i: usize = 0;
        while i + 16 <= bytes.len() {
            let chunk = _mm_loadu_si128(bytes.as_ptr().add(i) as *const _);
            let eq_q = _mm_cmpeq_epi8(chunk, quote);
            let eq_b = _mm_cmpeq_epi8(chunk, bslash);
            let mask = _mm_or_si128(eq_q, eq_b);
            let bitmask = _mm_movemask_epi8(mask) as u32;
            if bitmask != 0 {
                return Some(i + bitmask.trailing_zeros() as usize);
            }
            i += 16;
        }
        // Tail.
        find_string_terminator_scalar(&bytes[i..]).map(|off| i + off)
    }
}

// ─── Direct JSON parser ────────────────────────────────────────────────────────

/// Result of parsing a JSON string: either a zero-copy borrow from the
/// input buffer (no escapes) or an owned allocation (had escape sequences).
enum ParsedStr<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> ParsedStr<'a> {
    fn as_bytes(&self) -> &[u8] {
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
struct ObjectShapeHint {
    /// Pre-interned key pointers in declared field order. Pointers
    /// are held alive by PARSE_KEY_CACHE + scan_parse_roots.
    expected_keys: Vec<*const StringHeader>,
    /// Pre-built keys_array that each parsed record's ObjectHeader
    /// points to. Built via `js_build_class_keys_array`, so the
    /// shape cache + scan_shape_cache_roots keeps it alive.
    keys_array: *mut crate::array::ArrayHeader,
    /// Number of fields in the declared shape — used as the object's
    /// pre-allocated field count.
    field_count: u32,
}

struct DirectParser<'a> {
    input: &'a [u8],
    pos: usize,
    /// Issue #179 typed-parse: if Some, the top-level value is
    /// expected to be `Array<Object>` matching this shape. Each
    /// record uses the fast path; mismatches silently fall through
    /// to the generic field-setting logic.
    shape: Option<ObjectShapeHint>,
}

impl<'a> DirectParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            shape: None,
        }
    }

    fn with_shape(input: &'a [u8], shape: ObjectShapeHint) -> Self {
        Self {
            input,
            pos: 0,
            shape: Some(shape),
        }
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
    fn parse_string_bytes(&mut self) -> Option<ParsedStr<'a>> {
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

    fn parse_string_bytes_slow(&mut self, start: usize) -> Option<ParsedStr<'a>> {
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
    unsafe fn parse_object_shaped(&mut self, shape: &ObjectShapeHint) -> JSValue {
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
        let alloc_field_count = std::cmp::max(shape.field_count as usize, 8);
        for i in 0..alloc_field_count {
            let fields_ptr =
                (js_obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut JSValue;
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
            parse_root_push(value);
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
                                let fields_ptr = (js_obj as *mut u8)
                                    .add(std::mem::size_of::<crate::ObjectHeader>())
                                    as *mut JSValue;
                                let slot = fields_ptr.add(slot_idx);
                                std::ptr::write(slot, JSValue::from_bits(value_bits));
                                crate::gc::layout_note_slot(js_obj as usize, slot_idx, value_bits);
                                crate::gc::runtime_write_barrier_slot(
                                    js_obj as usize,
                                    slot as usize,
                                    value_bits,
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
    unsafe fn parse_array_typed(&mut self) -> JSValue {
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
        let mut js_arr = js_array_alloc(16);
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
            parse_root_push(value);
            js_arr = js_array_push(js_arr, value);
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
    unsafe fn parse_value_generic(&mut self) -> JSValue {
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

    unsafe fn parse_object(&mut self) -> JSValue {
        // The top-level entry `parse_value` routes typed-array parses
        // to `parse_array_typed` directly, so by the time we reach
        // `parse_object` here the only callers are (a) untyped parses
        // and (b) nested objects inside a shaped record — both want
        // generic behavior. Delegate to `parse_object_untyped`.
        self.parse_object_untyped()
    }

    unsafe fn parse_object_untyped(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();

        if self.peek() == Some(b'}') {
            self.advance();
            let keys: [*const StringHeader; 0] = [];
            let keys_arr = parse_shape_keys_array(&keys);
            let js_obj = crate::object::js_object_alloc_class_inline_keys(0, 0, 0, keys_arr);
            let fields_ptr =
                (js_obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut JSValue;
            for i in 0..8 {
                std::ptr::write(fields_ptr.add(i), JSValue::undefined());
            }
            parse_root_restore(saved_roots);
            return JSValue::object_ptr(js_obj as *mut u8);
        }

        let mut keys: Vec<*const StringHeader> = Vec::new();
        let mut values: Vec<JSValue> = Vec::new();

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
            // Root the value before the key-intern + set_field path
            // (which may allocate and trigger GC).
            parse_root_push(value);

            let key_bytes = key.as_bytes();
            let key_ptr = cached_parse_key_ptr(key_bytes);
            if let Some(existing) = keys.iter().position(|&ptr| ptr == key_ptr) {
                values[existing] = value;
            } else {
                keys.push(key_ptr);
                values.push(value);
            }

            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(b'}');
        let field_count = keys.len() as u32;
        let keys_arr = parse_shape_keys_array(&keys);
        let js_obj = crate::object::js_object_alloc_class_inline_keys(0, 0, field_count, keys_arr);
        let alloc_field_count = std::cmp::max(field_count as usize, 8);
        let fields_ptr =
            (js_obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            std::ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        for (i, value) in values.iter().enumerate() {
            let value_bits = value.bits();
            let slot = fields_ptr.add(i);
            std::ptr::write(slot, JSValue::from_bits(value_bits));
            crate::gc::layout_note_slot(js_obj as usize, i, value_bits);
            crate::gc::runtime_write_barrier_slot(js_obj as usize, slot as usize, value_bits);
        }
        parse_root_restore(saved_roots);
        JSValue::object_ptr(js_obj as *mut u8)
    }

    unsafe fn parse_array(&mut self) -> JSValue {
        self.advance();
        self.skip_whitespace();

        let saved_roots = parse_root_save_len();
        let mut js_arr = js_array_alloc(16);
        let arr_slot = parse_root_push(JSValue::object_ptr(js_arr as *mut u8));

        if self.peek() == Some(b']') {
            self.advance();
            parse_root_restore(saved_roots);
            return JSValue::object_ptr(js_arr as *mut u8);
        }

        loop {
            let value = self.parse_value();
            js_arr = parse_root_array_ptr(arr_slot);
            // Root value before push — js_array_push may grow (arena alloc → GC)
            // and value's heap ptr lives only in a caller-saved register here.
            parse_root_push(value);
            js_arr = js_array_push(js_arr, value);
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

    unsafe fn parse_number(&mut self) -> JSValue {
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

        // Fallthrough: consume any fractional + exponent parts, then
        // hand off to the general parser.
        if has_dot {
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

// ─── JSON.parse ───────────────────────────────────────────────────────────────

/// JSON.parse(text) shim that returns `null` for a null input instead
/// of throwing `Unexpected end of JSON input`. Used by codegen dispatch
/// rows whose runtime FFI returns `*mut StringHeader` containing JSON
/// for the success path and a null pointer for the failure path (e.g.
/// `jwt.verify` on a bad signature). User code expects a null-return,
/// not an uncaught exception that aborts the process. Issue #927.
///
/// # Safety
///
/// `text_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_or_null(text_ptr: *const StringHeader) -> JSValue {
    if text_ptr.is_null() {
        return JSValue::null();
    }
    js_json_parse(text_ptr)
}

/// JSON.parse(text) -> any
///
/// Uses a direct recursive-descent parser that constructs Perry JSValues
/// without any intermediate representation.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse(text_ptr: *const StringHeader) -> JSValue {
    if text_ptr.is_null() {
        let msg = "Unexpected end of JSON input";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_val = JSValue::string_ptr(msg_ptr);
        crate::exception::js_throw(f64::from_bits(err_val.bits()));
    }
    let len = (*text_ptr).byte_len as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    crate::gc::gc_collect_pending_suppressed_parse();

    // Issue #179 Step 2 Phase 1 → default-on: tape-based lazy parse
    // is now the default for top-level arrays on blobs larger than
    // the size threshold. v0.5.209 runtime adaptive handling (walk
    // cursor + cumulative-walk threshold + sparse cache + force-
    // materialize-on-mutate) means lazy is safe for non-tiny blobs.
    // The lower bound keeps tape-build overhead off sub-1 KB parses;
    // #437 briefly raised it for a 21 KB iterate-all fixture, but
    // #1090 restores 1 KB so RSS-sensitive parse-churn loops use the
    // tape path once payloads are larger than the direct tiny case.
    //
    // Escape hatches: `PERRY_JSON_TAPE=0` forces the direct parser
    // for every parse (correctness fallback if a workload hits an
    // unaudited code path on the lazy side). `PERRY_JSON_TAPE=1`
    // forces tape for every parse including small ones (useful for
    // testing). Any other value is treated as "auto" (the default).
    // Lazy parse is a win on workloads that touch only a subset of
    // a parsed top-level array — the tape build cost is ~one-shot
    // O(n) but each unread element saves the full subtree
    // materialization. For workloads that iterate the whole array
    // (the canonical "filter all records, stringify the result"
    // shape — `benchmarks/honest_bench/workloads/1_json_pipeline`
    // for example), lazy is strictly slower than direct: tape walk
    // + sparse-cache management on every access plus a forced
    // materialize at the end is more work than the direct parser's
    // single tree build. The cumulative walk-steps trigger in
    // `lazy_get` only catches *random* access, not sequential.
    //
    // The auto-mode size window: lazy fires at 1 KB and above (tiny
    // parses don't pay the tape build) and below 16 MB (very large
    // blobs are dominated by the iterate-all idiom in practice —
    // 108 MB honest_bench full fixture, server log dumps, dataset
    // ETL — and the direct parser's tree-build is faster end-to-
    // end than tape + materialize). The upper bound keeps those
    // very large cases direct while the restored lower bound keeps
    // JSON churn bounded for sub-collector-scale payloads.
    //
    // Escape hatch via PERRY_JSON_TAPE=1 (force lazy regardless
    // of size, useful for testing) / =0 (force direct, useful as
    // a correctness fallback).
    const LAZY_MIN_BLOB_BYTES: usize = 1024;
    const LAZY_MAX_BLOB_BYTES: usize = 16 * 1024 * 1024;
    let tape_mode = tape_mode_from_env();
    let use_tape = match tape_mode {
        TapeMode::ForceOn => true,
        TapeMode::ForceOff => false,
        TapeMode::Auto => len >= LAZY_MIN_BLOB_BYTES && len <= LAZY_MAX_BLOB_BYTES,
    };
    if use_tape {
        if let Some(result) = try_parse_via_tape(text_ptr, bytes) {
            return result;
        }
        // Malformed input or non-array top-level — fall through to
        // direct parser, which has the full error-reporting path
        // and handles non-array roots.
    }

    if len == 0 {
        let msg = "Unexpected end of JSON input";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_val = JSValue::string_ptr(msg_ptr);
        crate::exception::js_throw(f64::from_bits(err_val.bits()));
    }

    // #64 follow-up: opportunistic pre-parse cleanup. When parse runs in a
    // tight loop (e.g. `for (let i=0; i<N; i++) JSON.parse(blob);`), each
    // call suppresses GC for its duration, and the post-parse malloc-trigger
    // bump defers collection of the PREVIOUS iteration's now-dead tree.
    // Garbage accumulates until the arena trigger fires — typically during
    // the next stringify, producing one 100ms+ pause that walks every dead
    // block from every iteration. Calling `gc_check_trigger` here (before
    // suppression) lets the trigger fire normally between iterations so
    // garbage is shed incrementally. The new <10%-freed-doubles-step rule
    // in `gc_check_trigger` protects adversarial cases (previous stringify
    // result strings sharing blocks with interned keys) from retrigger
    // thrash when block-persistence keeps everything alive.
    crate::gc::gc_check_trigger();

    // Suppress GC for the duration of the parse. Parse is synchronous and
    // roots all intermediates in PARSE_ROOTS, so no collection is needed
    // until we're done. This eliminates O(n*m) overhead from mid-parse GC
    // cycles walking an ever-growing live set (issue #59).
    crate::gc::gc_suppress();

    let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));

    let mut parser = DirectParser::new(bytes);
    let result = parser.parse_value();
    parse_root_push(result);

    // Re-enable GC and rebaseline triggers while the result is still
    // rooted. Tiny parse-churn pressure may collect here; keeping the
    // parse roots until after the bump protects the value being returned.
    crate::gc::gc_unsuppress();
    crate::gc::gc_bump_malloc_trigger();
    crate::gc::gc_schedule_parse_boundary_collection_if_pressure();
    parse_root_restore(text_root);

    // Keep key intern cache across parses — scan_parse_roots marks cached
    // strings as GC roots so they survive collection. This saves ~10k
    // gc_malloc calls per repeated parse of homogeneous JSON (same keys).
    // Cap at 4096 entries to bound memory for varied-schema workloads.
    PARSE_KEY_CACHE.with(|c| {
        let cache = c.borrow();
        if cache.len() > 4096 {
            drop(cache);
            c.borrow_mut().clear();
        }
    });

    // If parser didn't consume meaningful input (result is null and input wasn't "null"),
    // the input was invalid JSON — throw SyntaxError
    if result.is_null() {
        let is_literal_null = len >= 4 && bytes.starts_with(b"null");
        if !is_literal_null {
            let preview_len = len.min(50);
            let preview = std::str::from_utf8(&bytes[..preview_len]).unwrap_or("???");
            let msg = format!("JSON parse error: Unexpected token: {}", preview);
            let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err_val = JSValue::string_ptr(msg_ptr);
            crate::exception::js_throw(f64::from_bits(err_val.bits()));
        }
    }

    result
}

/// v0.5.210: tape-mode selector. Cached at first JSON.parse so we
/// pay the env-var lookup once per process, not once per parse.
#[derive(Copy, Clone)]
enum TapeMode {
    Auto,
    ForceOn,
    ForceOff,
}

/// SSO Step 1 test gate. `PERRY_SSO_FORCE=1` (or `on`/`true`) flips
/// `DirectParser::parse_string_value` to emit inline SSO values for
/// strings of length ≤ 5. Used by the migration test suite to
/// exercise every stringify / equality / compare consumer arm
/// across both representations. Cached so the per-parse-call cost
/// is one relaxed atomic load.
fn sso_emit_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_SSO_FORCE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

fn tape_mode_from_env() -> TapeMode {
    use std::sync::OnceLock;
    static CACHED: OnceLock<TapeMode> = OnceLock::new();
    *CACHED.get_or_init(|| match std::env::var("PERRY_JSON_TAPE").as_deref() {
        Ok("0") | Ok("off") | Ok("false") => TapeMode::ForceOff,
        Ok("1") | Ok("on") | Ok("true") => TapeMode::ForceOn,
        _ => TapeMode::Auto,
    })
}

/// Issue #179 Step 2 Phase 1: tape-path entry. Builds a tape from
/// the input bytes, then materializes the full JSValue tree via
/// `json_tape::materialize`. Returns `None` on malformed input so
/// the caller can fall through to the direct parser.
///
/// Wraps the tape path in the same GC-safety contract as the direct
/// parser (pending parse-boundary collection → gc_check_trigger →
/// suppress → parse → unsuppress → bump malloc trigger + cache trim) so
/// it's a drop-in replacement behind the feature flag.
unsafe fn try_parse_via_tape(text_ptr: *const StringHeader, bytes: &[u8]) -> Option<JSValue> {
    crate::json_tape::with_built_tape(bytes, |tape_entries| {
        crate::gc::gc_collect_pending_suppressed_parse();
        crate::gc::gc_check_trigger();
        crate::gc::gc_suppress();
        let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));

        // Phase 2: if the top-level value is an array, return a lazy
        // array header instead of materializing the tree. Every other
        // shape (objects, scalars) still materializes eagerly — this
        // commit's scope is top-level arrays only (the shape that
        // dominates `bench_json_roundtrip` and most realistic JSON.parse
        // workloads). Extending to top-level objects in a follow-up is a
        // straightforward mirror of the same construction.
        let result = if !tape_entries.is_empty()
            && tape_entries[0].kind == crate::json_tape::KIND_ARR_START
        {
            let len = crate::json_tape::count_array_length(tape_entries, 0);
            let hdr = crate::json_tape::alloc_lazy_array(tape_entries, 0, len, text_ptr);
            JSValue::object_ptr(hdr as *mut u8)
        } else {
            crate::json_tape::materialize_from_idx(tape_entries, bytes, 0)
        };
        parse_root_push(result);

        crate::gc::gc_unsuppress();
        crate::gc::gc_bump_malloc_trigger();
        parse_root_restore(text_root);

        PARSE_KEY_CACHE.with(|c| {
            let cache = c.borrow();
            if cache.len() > 4096 {
                drop(cache);
                c.borrow_mut().clear();
            }
        });

        result
    })
}

// ─── JSON.parse<T[]>: schema-directed typed parse ─────────────────────────────

/// Issue #179 typed-parse plan, Step 1b. Entry point for
/// `JSON.parse<T[]>(blob)` where T is an object type whose field names
/// are known at codegen time.
///
/// `packed_keys` is null-separated UTF-8 field names in declared order:
/// `b"id\0name\0value\0"`. `field_count` is the number of fields
/// (== number of `\0` separators).
///
/// Runtime behavior is identical to `js_json_parse(text_ptr)` —
/// semantically the same JSON, same JSValue tree, same Node parity.
/// The specialization just skips:
/// - Per-record shape-cache lookup (shape built once per call)
/// - Per-field `PARSE_KEY_CACHE` hash when fields arrive in declared
///   order (the common case for stringify output and most machine-
///   generated JSON)
/// - Per-field transition-cache dance inside `js_object_set_field_by_name`
///   for in-order fields (direct field-index write)
///
/// Out-of-order, extra, or missing fields all fall through to the
/// generic named-setter path — correctness-preserving.
///
/// On input shape mismatch (top-level isn't an array, records aren't
/// objects), also falls through to the generic parser. No user-
/// visible difference from `JSON.parse(blob) as T[]`.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_typed_array(
    text_ptr: *const StringHeader,
    packed_keys: *const u8,
    packed_keys_len: u32,
    field_count: u32,
) -> JSValue {
    if text_ptr.is_null() {
        // Fall through to generic (which will throw the standard error).
        return js_json_parse(text_ptr);
    }
    let len = (*text_ptr).byte_len as usize;
    if len == 0 {
        return js_json_parse(text_ptr);
    }
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    // Build the shape hint once. The keys_array + pre-interned key
    // pointers are owned by longlived arena + shape-cache structures,
    // so they outlive the parse and survive any intervening GC.
    let shape = match build_shape_hint(packed_keys, packed_keys_len, field_count) {
        Some(s) => s,
        None => return js_json_parse(text_ptr),
    };

    // Same pre-parse cleanup + GC suppression as `js_json_parse` —
    // keeps the typed path on the same GC-safety contract.
    crate::gc::gc_collect_pending_suppressed_parse();
    crate::gc::gc_check_trigger();
    crate::gc::gc_suppress();
    let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));

    let mut parser = DirectParser::with_shape(bytes, shape);
    let result = parser.parse_array_typed();
    parse_root_push(result);

    crate::gc::gc_unsuppress();
    crate::gc::gc_bump_malloc_trigger();
    parse_root_restore(text_root);

    PARSE_KEY_CACHE.with(|c| {
        let cache = c.borrow();
        if cache.len() > 4096 {
            drop(cache);
            c.borrow_mut().clear();
        }
    });

    if result.is_null() {
        let is_literal_null = len >= 4 && bytes.starts_with(b"null");
        if !is_literal_null {
            let preview_len = len.min(50);
            let preview = std::str::from_utf8(&bytes[..preview_len]).unwrap_or("???");
            let msg = format!("JSON parse error: Unexpected token: {}", preview);
            let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err_val = JSValue::string_ptr(msg_ptr);
            crate::exception::js_throw(f64::from_bits(err_val.bits()));
        }
    }

    result
}

/// Build the one-per-call shape hint: intern key strings into
/// `PARSE_KEY_CACHE` (longlived arena) and build a shared
/// `keys_array` via the existing `js_build_class_keys_array` path so
/// `scan_shape_cache_roots` keeps it marked. Returns `None` if
/// `packed_keys` is malformed (no separators, unexpected count).
unsafe fn build_shape_hint(
    packed_keys: *const u8,
    packed_keys_len: u32,
    field_count: u32,
) -> Option<ObjectShapeHint> {
    if packed_keys.is_null() || field_count == 0 {
        return None;
    }
    let packed = std::slice::from_raw_parts(packed_keys, packed_keys_len as usize);
    // Same parsing as `js_build_class_keys_array`: split on `\0`,
    // drop empties.
    let keys: Vec<&[u8]> = packed
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    if keys.len() != field_count as usize {
        return None;
    }
    // Intern each key via PARSE_KEY_CACHE so the pointers are shared
    // with the generic-parse path — critical for the transition cache
    // to treat them as identical during slow-path field sets.
    let mut expected_keys: Vec<*const StringHeader> = Vec::with_capacity(keys.len());
    for key_bytes in &keys {
        let cached = PARSE_KEY_CACHE.with(|c| c.borrow().get(*key_bytes).copied());
        let ptr = if let Some(p) = cached {
            p
        } else {
            let p = crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(),
                key_bytes.len() as u32,
            );
            PARSE_KEY_CACHE.with(|c| {
                c.borrow_mut().insert(key_bytes.to_vec(), p);
            });
            p
        };
        expected_keys.push(ptr);
    }

    // Build the keys_array via the existing class-shape path. We
    // derive a class_id by hashing packed_keys so repeated typed-parse
    // calls with the same shape reuse the same keys_array (cache hit).
    let class_id = shape_hash(packed) as u32;
    let keys_array = crate::object::js_build_class_keys_array(
        class_id,
        field_count,
        packed_keys,
        packed_keys_len,
    );

    Some(ObjectShapeHint {
        expected_keys,
        keys_array,
        field_count,
    })
}

#[inline]
fn shape_hash(bytes: &[u8]) -> u64 {
    // FNV-1a, matching the style Perry uses elsewhere for shape
    // identity. A collision just means two distinct shapes share a
    // class_id in the shape cache — the cache is content-compared on
    // miss so no correctness issue, just a modest re-build cost.
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Nonzero class_id (0 is reserved for plain objects).
    h | 0x8000_0000_0000_0000
}

// ─── JSON.stringify ───────────────────────────────────────────────────────────

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
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

/// Read the GC header's object type tag for a user-space heap pointer.
/// The GcHeader sits 8 bytes before `ptr`; its first byte is `obj_type`.
/// Returns 0 when `ptr` is null or in the low-memory guard range.
#[inline]
unsafe fn gc_obj_type(ptr: *const u8) -> u8 {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return 0;
    }
    // GcHeader.obj_type is at offset 0 (see crate::gc::GcHeader layout).
    *(ptr.sub(crate::gc::GC_HEADER_SIZE))
}

/// Issue #639: emit a Buffer / Uint8Array as JSON in the Node-compatible shape.
///
/// `Buffer.from(...)` returns `{"type":"Buffer","data":[b0,b1,...]}` (Node's
/// `Buffer.prototype.toJSON()` output). `new Uint8Array(...)` returns
/// `{"0":b0,"1":b1,...}` (the typed-array shape Node falls through to with
/// no custom `toJSON`). Distinguished via `is_uint8array_buffer`, which the
/// Uint8Array constructor path explicitly marks (see `buffer.rs::js_uint8array_*`).
///
/// Must be called BEFORE `gc_obj_type(ptr)` — `BufferHeader` has no `GcHeader`,
/// so reading 8 bytes before the header reads unrelated memory and would
/// dispatch to the wrong arm (or panic when `is_object_pointer` deref's a
/// bogus `keys_array` pointer).
unsafe fn stringify_buffer(ptr: *const u8, buf: &mut String) {
    let buf_ptr = ptr as *const crate::buffer::BufferHeader;
    if buf_ptr.is_null() {
        buf.push_str("null");
        return;
    }
    let len = (*buf_ptr).length as usize;
    let data = (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);

    if crate::buffer::is_uint8array_buffer(ptr as usize) {
        buf.push('{');
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 {
                buf.push(',');
            }
            buf.push('"');
            let mut idx_buf = itoa::Buffer::new();
            buf.push_str(idx_buf.format(i));
            buf.push_str("\":");
            let mut byte_buf = itoa::Buffer::new();
            buf.push_str(byte_buf.format(*b));
        }
        buf.push('}');
    } else {
        buf.push_str(r#"{"type":"Buffer","data":["#);
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 {
                buf.push(',');
            }
            let mut byte_buf = itoa::Buffer::new();
            buf.push_str(byte_buf.format(*b));
        }
        buf.push_str("]}");
    }
}

#[inline]
unsafe fn is_object_pointer(ptr: *const u8) -> bool {
    let obj = ptr as *const crate::ObjectHeader;
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
        // keys_len is authoritative — the logical property count. field_count
        // can be EITHER less than keys_len (parser-built objects with ≥9
        // fields cap field_count at the inline alloc_limit; closes #307;
        // overflow values live in OVERFLOW_FIELDS — see object.rs:32) OR
        // greater than keys_len (pre-allocated objects like
        // `js_object_alloc(0, 8)` for 2 actual keys). Both shapes are real
        // objects worth stringifying; just sanity-check both fields are
        // within reasonable bounds.
        // Previously caps were `< 1000` — any object with 1000+ keys
        // failed the check and `JSON.stringify` emitted "null". Raised
        // to 10M which still catches a corrupted ObjectHeader (first-
        // fields bytes reading as 0x4059... — orders of magnitude
        // above 10M) but allows realistic object sizes through.
        keys_len <= keys_cap && keys_len > 0 && keys_cap < 10_000_000 && field_count < 10_000_000
    } else {
        false
    }
}

#[inline]
unsafe fn write_number(buf: &mut String, value: f64) {
    // Date — JSON.stringify must call toJSON/toISOString per ECMA-262
    // 24.5.4.1. Perry stores Date as a raw f64 timestamp; the
    // `DATE_REGISTRY` HashSet records which finite numbers came from a
    // `new Date(...)`. Every numeric write that's actually a Date routes
    // here as the last-resort fallback, so centralizing the check at
    // this single funnel covers all template / fast-path / replacer
    // sites without per-call-site changes. Guarded on "finite integer"
    // first since the lookup involves a thread-local HashSet — only
    // millisecond-shaped values can be Dates.
    if value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value < 1e16 {
        if crate::date::is_registered_date_bits(value.to_bits()) {
            let s_ptr = crate::date::js_date_to_iso_string(value);
            if let Some(s) = str_from_header(s_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
            return;
        }
    }
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
    let bytes = s.as_bytes();
    // Fast path: scan for any escape-triggering byte. JSON output is
    // overwhelmingly escape-free (ASCII identifiers, simple values), so
    // a straight-line SIMD-friendly scan + one `push_str` beats the
    // scalar per-byte escape loop. Needs_escape fires for `"`, `\`, or
    // any control byte (< 0x20).
    let needs_escape = bytes.iter().any(|&b| b < 0x20 || b == b'"' || b == b'\\');
    if !needs_escape {
        buf.reserve(bytes.len() + 2);
        buf.push('"');
        buf.push_str(s);
        buf.push('"');
        return;
    }

    buf.push('"');
    let mut start = 0;
    // Issue #548: `s` reaches us via `str_from_header`, which uses
    // `from_utf8_unchecked` — a misclassified pointer (e.g. an
    // ArrayHeader interpreted as a StringHeader through the GC-type
    // fallback heuristic) can produce a `&str` whose bytes are not
    // valid UTF-8. The original `&s[start..i]` slice operation
    // panics in `core::str::is_char_boundary` whenever `i` lands
    // mid-multibyte (or on a stray continuation byte). Switching to
    // byte-level `extend_from_slice` writes the raw bytes through and
    // never inspects char boundaries; the JSON output stays
    // byte-identical for valid UTF-8 inputs and degrades gracefully
    // (non-UTF-8 bytes pass through verbatim) instead of aborting the
    // whole process. The String we hand back is technically
    // ill-formed in the worst case, but every consumer in this
    // codebase treats stringify output as a byte stream — and an
    // ill-formed result is strictly preferable to a SIGABRT.
    let buf_vec = buf.as_mut_vec();
    for (i, &b) in bytes.iter().enumerate() {
        let escape = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0..=0x1f => {
                if start < i {
                    buf_vec.extend_from_slice(&bytes[start..i]);
                }
                buf_vec.extend_from_slice(format!("\\u{:04x}", b).as_bytes());
                start = i + 1;
                continue;
            }
            _ => None,
        };
        if let Some(esc) = escape {
            if start < i {
                buf_vec.extend_from_slice(&bytes[start..i]);
            }
            buf_vec.extend_from_slice(esc.as_bytes());
            start = i + 1;
        }
    }
    if start < bytes.len() {
        buf_vec.extend_from_slice(&bytes[start..]);
    }
    buf_vec.push(b'"');
}

/// Check if a NaN-boxed value is a closure (function).
#[inline]
unsafe fn is_closure_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        // Check for ClosureHeader magic at offset 8 (type_tag field)
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        type_tag == crate::closure::CLOSURE_MAGIC
    } else {
        false
    }
}

/// Check if an object has a toJSON method. If so, call it and return the result as f64.
/// Returns None if no toJSON method exists.
#[inline]
unsafe fn object_get_to_json(ptr: *const u8) -> Option<f64> {
    let obj = ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;

    for f in 0..keys_len {
        let key_f64 = *keys_elements.add(f as usize);
        let key_bits = key_f64.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };
        if let Some(key_str) = str_from_header(key_ptr) {
            if key_str == "toJSON" {
                let field_val = *fields_ptr.add(f as usize);
                let field_bits = field_val.to_bits();
                // Check if this field is a closure
                if is_closure_value(field_bits) {
                    let closure_ptr = if (field_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
                        (field_bits & POINTER_MASK) as *const crate::closure::ClosureHeader
                    } else {
                        field_bits as *const crate::closure::ClosureHeader
                    };
                    // Call toJSON() with no arguments (pass empty string key per spec)
                    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
                    let key_f64_arg =
                        f64::from_bits(STRING_TAG | (empty_str as u64 & POINTER_MASK));
                    let result = crate::js_closure_call1(closure_ptr, key_f64_arg);
                    return Some(result);
                }
            }
        }
    }
    None
}

#[inline]
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
    // SSO (v0.5.213): decode inline 5-byte string, emit escaped.
    if tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // BigInt: serialize as quoted string (matching JSON.stringify with BigInt replacer behavior)
    if tag == BIGINT_TAG {
        let bigint_ptr = (bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
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

        // Issue #639: Buffer/Uint8Array detection BEFORE gc_obj_type —
        // BufferHeader has no GcHeader, so the gc-tag read would read
        // unrelated memory and the resulting dispatch could segfault on
        // is_object_pointer's `keys_array` deref.
        if crate::buffer::is_registered_buffer(ptr as usize) {
            stringify_buffer(ptr, buf);
            return;
        }

        // Prefer the GC header's obj_type tag for dispatch — the old
        // capacity heuristic (`cap < 10000`) misidentified legitimate
        // arrays that had grown past 10k as strings, panicking on
        // `JSON.stringify(arr)` where `arr.length >= 10000` (issue #43).
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_ARRAY => stringify_array(ptr, buf),
            crate::gc::GC_TYPE_OBJECT => {
                if is_object_pointer(ptr) {
                    stringify_object(ptr, buf);
                } else {
                    buf.push_str("null");
                }
            }
            crate::gc::GC_TYPE_STRING => {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
            crate::gc::GC_TYPE_ERROR => {
                // Issue #928: Built-in Error objects (and subclasses
                // like TypeError) have a dedicated `ErrorHeader` layout —
                // not the JSObject keys/values layout. Routing them
                // through `stringify_object` derefs garbage as a
                // `keys_array` pointer and segfaults the process.
                // Node's `JSON.stringify(new Error("x"))` returns "{}"
                // because Error's intrinsic props (`message`, `name`,
                // `stack`) are non-enumerable; mirror that.
                buf.push_str("{}");
            }
            _ => {
                // Unknown/untagged pointer: fall back to the structural
                // heuristics for safety (e.g. pointers to non-GC-tracked
                // memory). Arrays up to 10k cap are dispatched here;
                // above that we defensively emit "null" rather than
                // trying to treat them as strings.
                if is_object_pointer(ptr) {
                    stringify_object(ptr, buf);
                } else {
                    let arr = ptr as *const crate::ArrayHeader;
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
            }
        }
        return;
    }

    write_number(buf, value);
}

/// Depth-aware stringify for recursive calls from stringify_object_inner.
/// For non-pointer values this is identical to stringify_value; for
/// objects/arrays it threads the depth counter through.
#[inline]
unsafe fn stringify_value_depth(value: f64, type_hint: u32, buf: &mut String, depth: u32) {
    let bits: u64 = value.to_bits();

    // Fast path: non-pointer values don't recurse
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
    // SSO (v0.5.213): decode inline 5-byte string, emit escaped.
    if tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if tag == BIGINT_TAG {
        let bigint_ptr = (bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_OBJECT {
            stringify_object_inner(ptr, buf, depth);
            return;
        }
        if type_hint == TYPE_ARRAY {
            stringify_array_depth(ptr, buf, depth);
            return;
        }
        // Issue #639: Buffer/Uint8Array detection BEFORE gc_obj_type — see
        // the matching branch in `stringify_value`.
        if crate::buffer::is_registered_buffer(ptr as usize) {
            stringify_buffer(ptr, buf);
            return;
        }
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_OBJECT => stringify_object_inner(ptr, buf, depth),
            crate::gc::GC_TYPE_ARRAY => stringify_array_depth(ptr, buf, depth),
            crate::gc::GC_TYPE_STRING => {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
            crate::gc::GC_TYPE_ERROR => {
                // Issue #928: see the matching branch in `stringify_value`.
                buf.push_str("{}");
            }
            _ => {
                if is_object_pointer(ptr) {
                    stringify_object_inner(ptr, buf, depth);
                } else {
                    let arr = ptr as *const crate::ArrayHeader;
                    if !arr.is_null() {
                        let len = (*arr).length;
                        let cap = (*arr).capacity;
                        if len <= cap && cap > 0 && cap < 10000 {
                            stringify_array_depth(ptr, buf, depth);
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
            }
        }
        return;
    }

    write_number(buf, value);
}

/// Stringify depth counter — avoids TLS `STRINGIFY_STACK` access for
/// shallow (non-circular) object graphs. Only activates full tracking
/// at depth > MAX_FAST_DEPTH to catch genuine circular refs.
const MAX_FAST_DEPTH: u32 = 128;

#[inline]
unsafe fn stringify_object(ptr: *const u8, buf: &mut String) {
    stringify_object_inner(ptr, buf, 0)
}

unsafe fn stringify_object_inner(ptr: *const u8, buf: &mut String, depth: u32) {
    if depth > MAX_FAST_DEPTH {
        // Deep nesting — switch to full circular detection
        if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
            let msg = "Converting circular structure to JSON";
            let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err_ptr = crate::error::js_typeerror_new(msg_ptr);
            crate::exception::js_throw(f64::from_bits(
                POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
            ));
        }
        STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));
    }

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;

    // Templated fast path (#64 follow-up): if this object's shape has been
    // seen before in this stringify call, emit via the cached prefix table
    // and skip per-object `has_pointer_fields` / `object_get_to_json` /
    // key-lookup work. `try_emit_shape_element` rolls back the buffer and
    // returns false on any element-specific mismatch (different shape,
    // stray UNDEFINED, closure), at which point we fall through to the
    // slow path below.
    //
    // Guard (issue #67): skip the template machinery for small objects.
    // `shape_template_for` allocates a Box<ShapeTemplate> + Vec<String>
    // + one String per field on miss (~4-5 heap allocs), and the cache
    // is wiped at every top-level call exit — so for a one-shot small
    // top-level stringify the build is pure overhead vs. the inline slow
    // path below. The arrayof-objects fast path (stringify_array_depth)
    // uses a separate build_shape_prefix_template that's unaffected.
    // Skip the shape-template fast path when the object has overflow fields
    // (keys_len > num_fields — see object.rs:32 OVERFLOW_FIELDS, ≥9 stored
    // fields per #307). The template's per-field key prefix array is built
    // from `min(keys_len, field_count)`, so an overflow object would only
    // emit its first 8 fields. Falling through to the slow path below uses
    // `read_field_bits` which routes overflow reads through
    // `js_object_get_field`'s overflow_get fallback.
    let has_overflow_fields = unsafe {
        let keys_arr = (*obj).keys_array;
        !keys_arr.is_null() && (*keys_arr).length > num_fields
    };
    if num_fields >= 5 && !has_overflow_fields {
        if let Some(tmpl_ptr) = shape_template_for(ptr) {
            if try_emit_shape_element(make_pointer_bits(ptr), &*tmpl_ptr, buf, depth) {
                if depth > MAX_FAST_DEPTH {
                    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
                }
                return;
            }
        }
    }
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    // Closes #307: iterate up to keys_len, not min(num_fields, keys_len).
    // Parser-built objects with ≥9 fields cap field_count at the inline
    // alloc_limit (max(field_count, 8) physical slots) and store the overflow
    // values in OVERFLOW_FIELDS (object.rs:32) — so num_fields can be smaller
    // than keys_len. For inline slots (f < alloc_limit) we still read directly
    // off fields_ptr; for overflow slots we route through `js_object_get_field`
    // which checks field_count and falls through to `overflow_get`. Pre-fix
    // (`std::cmp::min(num_fields, keys_len)`) silently dropped the overflow
    // fields and `is_object_pointer`'s `keys_len <= field_count` guard
    // returned false, so `JSON.stringify` emitted the literal string "null"
    // for any parsed object with ≥9 fields.
    let alloc_limit = std::cmp::max(num_fields, 8);
    let read_field_bits = |f: u32| -> u64 {
        if f < alloc_limit {
            (*fields_ptr.add(f as usize)).to_bits()
        } else {
            crate::object::js_object_get_field(obj, f).bits()
        }
    };
    let actual_fields = keys_len;

    // Deferred toJSON + closure checks (issue #67 tightening): scan fields
    // once to detect if any field is actually a closure. For data-only
    // objects with nested arrays/objects (e.g. `{a:1, b:"", c:[...]}`) the
    // earlier has_pointer_fields heuristic false-positived because any
    // POINTER_TAG field triggered the `object_get_to_json` key walk — even
    // though a toJSON method requires the *value* at the "toJSON" key to
    // be a closure. Reading offset 12 (CLOSURE_MAGIC) per pointer field is
    // cheaper (~3ns/field) than walking the keys array looking for a
    // "toJSON" string that almost never exists (~15ns).
    let has_closure_field = {
        let mut found = false;
        for f in 0..actual_fields {
            let bits = read_field_bits(f);
            let tag = bits & 0xFFFF_0000_0000_0000;
            let ptr_candidate = if tag == POINTER_TAG {
                (bits & POINTER_MASK) as *const u8
            } else if is_raw_pointer(bits) {
                bits as *const u8
            } else {
                std::ptr::null()
            };
            if !ptr_candidate.is_null() {
                let type_tag = *(ptr_candidate.add(12) as *const u32);
                if type_tag == crate::closure::CLOSURE_MAGIC {
                    found = true;
                    break;
                }
            }
        }
        found
    };

    if has_closure_field {
        // Only objects with closure-typed fields can have a toJSON method.
        // Check toJSON first, then filter closures in the loop below.
        if let Some(to_json_val) = object_get_to_json(ptr) {
            if depth > MAX_FAST_DEPTH {
                STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
            }
            stringify_value(to_json_val, TYPE_UNKNOWN, buf);
            return;
        }
    }

    buf.push('{');
    let mut first = true;
    for f in 0..actual_fields {
        let field_bits = read_field_bits(f);
        let field_val = f64::from_bits(field_bits);
        // Skip undefined per JSON spec
        if field_bits == TAG_UNDEFINED {
            continue;
        }
        // Skip closures per JSON spec (only possible for pointer-tagged values).
        // Guarded by has_closure_field: if no field is a closure, the in-loop
        // check is skipped entirely for every field.
        if has_closure_field && is_closure_value(field_bits) {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

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

        // Inline value dispatch for common types to avoid function call overhead
        let val_tag = field_bits & 0xFFFF_0000_0000_0000;
        if field_bits == TAG_NULL {
            buf.push_str("null");
        } else if field_bits == TAG_TRUE {
            buf.push_str("true");
        } else if field_bits == TAG_FALSE {
            buf.push_str("false");
        } else if val_tag == STRING_TAG {
            let str_ptr = (field_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if val_tag == crate::value::SHORT_STRING_TAG {
            // v0.5.213 SSO — decode inline 5-byte string and emit.
            let jsval = JSValue::from_bits(field_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if val_tag == POINTER_TAG || is_raw_pointer(field_bits) {
            // Nested object/array — recurse with depth
            stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
        } else {
            // Number (most common for data objects) — or Date, handled
            // centrally by `write_number` via DATE_REGISTRY lookup.
            write_number(buf, field_val);
        }
    }
    buf.push('}');
    if depth > MAX_FAST_DEPTH {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
    }
}

unsafe fn stringify_array(ptr: *const u8, buf: &mut String) {
    stringify_array_depth(ptr, buf, 0)
}

/// Cached shape template for a homogeneous array of objects.
struct ShapeTemplate {
    keys_arr: *mut crate::ArrayHeader,
    prefixes: Vec<String>,
    shape_fields: u32,
    /// True when element 0's fields are all primitives (no POINTER_TAG /
    /// UNDEFINED). Lets the emit path skip its per-element pre-scan.
    primitive_only: bool,
}

/// Build a per-shape key-prefix template for a homogeneous array of objects.
///
/// When every element of an array shares the same `keys_array` pointer (same
/// shape), we can pre-format the key portion of each field once and reuse it
/// across every element — turning the per-field key lookup (load key f64,
/// extract pointer, `str_from_header`, 3 `push`/`push_str` calls) into a
/// single `push_str` of a cached prefix.
///
/// Prefix layout for N fields with keys k0..kN-1:
///   `prefixes[0]   = "{\"k0\":"`        (opening brace fused with first key)
///   `prefixes[f>0] = ",\"kf\":"`        (comma fused with key)
/// Close with `}`. This compresses ~7 per-field write ops down to ~2.
///
/// Returns `None` when the first element isn't a regular object, the keys
/// array is invalid, or any key string is malformed — callers fall back to
/// the generic slow path in that case.
unsafe fn build_shape_prefix_template(first_elem_bits: u64) -> Option<ShapeTemplate> {
    let tag = first_elem_bits & 0xFFFF_0000_0000_0000;
    let first_ptr = if tag == POINTER_TAG {
        (first_elem_bits & POINTER_MASK) as *const u8
    } else if is_raw_pointer(first_elem_bits) {
        first_elem_bits as *const u8
    } else {
        return None;
    };
    // Issue #639: Buffer / Uint8Array have no GcHeader, so `gc_obj_type`
    // would read 8 bytes before the BufferHeader (unrelated memory) and
    // could randomly return GC_TYPE_OBJECT. Bail to the per-element
    // slow path which dispatches via `is_registered_buffer`.
    if crate::buffer::is_registered_buffer(first_ptr as usize) {
        return None;
    }
    if gc_obj_type(first_ptr) != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = first_ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }
    let keys_len = (*keys_arr).length;
    let field_count = (*obj).field_count;
    let shape_fields = std::cmp::min(keys_len, field_count);
    if shape_fields == 0 || shape_fields > 32 {
        return None;
    }

    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut prefixes: Vec<String> = Vec::with_capacity(shape_fields as usize);
    for f in 0..shape_fields {
        let key_bits = (*keys_elements.add(f as usize)).to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };
        let key_str = str_from_header(key_ptr)?;
        let needs_escape = key_str.bytes().any(|b| b == b'"' || b == b'\\' || b < 0x20);
        let mut prefix = String::with_capacity(key_str.len() + 4);
        prefix.push(if f == 0 { '{' } else { ',' });
        if needs_escape {
            write_escaped_string(&mut prefix, key_str);
        } else {
            prefix.push('"');
            prefix.push_str(key_str);
            prefix.push('"');
        }
        prefix.push(':');
        prefixes.push(prefix);
    }

    // Sample first element to decide whether every field slot is already
    // a primitive (number/bool/null/string). When true, per-element emit
    // can skip the undefined/closure pre-scan.
    let fields_ptr =
        (first_ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let mut primitive_only = true;
    for f in 0..shape_fields {
        let fb = (*fields_ptr.add(f as usize)).to_bits();
        if fb == TAG_UNDEFINED || (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            primitive_only = false;
            break;
        }
    }

    Some(ShapeTemplate {
        keys_arr,
        prefixes,
        shape_fields,
        primitive_only,
    })
}

/// Fast emission path for an object element that matches the cached shape
/// template. Returns `true` when the element was emitted via the template;
/// `false` when the element diverges (different shape, skippable field, or
/// has a `toJSON` that must produce the replacement value). On `false` the
/// buffer is unchanged — the caller is responsible for falling back.
unsafe fn try_emit_shape_element(
    elem_bits: u64,
    template: &ShapeTemplate,
    buf: &mut String,
    depth: u32,
) -> bool {
    let tag = elem_bits & 0xFFFF_0000_0000_0000;
    let elem_ptr = if tag == POINTER_TAG {
        (elem_bits & POINTER_MASK) as *const u8
    } else if is_raw_pointer(elem_bits) {
        elem_bits as *const u8
    } else {
        return false;
    };
    if gc_obj_type(elem_ptr) != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let obj = elem_ptr as *const crate::ObjectHeader;
    if (*obj).keys_array != template.keys_arr {
        return false;
    }

    let fields_ptr =
        (elem_ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let shape_fields = template.shape_fields;
    let prefixes = template.prefixes.as_slice();

    // Primitive-only fast path (common case for JSON.parse output): skip
    // the undefined/closure pre-scan and trust that the sampled element 0
    // was representative. The emit loop handles stray POINTER_TAG values
    // via `stringify_value_depth`; a stray UNDEFINED is rare enough that
    // we save `buf.len()` pre-emit and roll back on detection.
    if template.primitive_only {
        let save_pos = buf.len();
        for f in 0..shape_fields as usize {
            let field_val = *fields_ptr.add(f);
            let fb = field_val.to_bits();
            // UNDEFINED desyncs comma placement → roll back and let the
            // slow object path emit this element correctly.
            if fb == TAG_UNDEFINED {
                buf.truncate(save_pos);
                return false;
            }
            buf.push_str(&prefixes[f]);
            let vtag = fb & 0xFFFF_0000_0000_0000;
            if fb == TAG_NULL {
                buf.push_str("null");
            } else if fb == TAG_TRUE {
                buf.push_str("true");
            } else if fb == TAG_FALSE {
                buf.push_str("false");
            } else if vtag == STRING_TAG {
                let str_ptr = (fb & POINTER_MASK) as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            } else if vtag == crate::value::SHORT_STRING_TAG {
                let jsval = JSValue::from_bits(fb);
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let n = jsval.short_string_to_buf(&mut scratch);
                if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            } else if vtag == POINTER_TAG || is_raw_pointer(fb) {
                stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
            } else {
                write_number(buf, field_val);
            }
        }
        buf.push('}');
        return true;
    }

    // General path: template contains (or may contain) pointer/undefined
    // fields. Pre-scan to honor JSON spec (skip undefined, skip closures,
    // respect toJSON).
    let mut has_pointer_fields = false;
    for f in 0..shape_fields as usize {
        let fb = (*fields_ptr.add(f)).to_bits();
        if fb == TAG_UNDEFINED {
            return false;
        }
        if (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            has_pointer_fields = true;
            if is_closure_value(fb) {
                return false;
            }
        }
    }
    if has_pointer_fields {
        if let Some(to_json_val) = object_get_to_json(elem_ptr) {
            stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
            return true;
        }
    }
    for f in 0..shape_fields as usize {
        buf.push_str(&prefixes[f]);
        let field_val = *fields_ptr.add(f);
        let fb = field_val.to_bits();
        let vtag = fb & 0xFFFF_0000_0000_0000;
        if fb == TAG_NULL {
            buf.push_str("null");
        } else if fb == TAG_TRUE {
            buf.push_str("true");
        } else if fb == TAG_FALSE {
            buf.push_str("false");
        } else if vtag == STRING_TAG {
            let str_ptr = (fb & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if vtag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(fb);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if vtag == POINTER_TAG || is_raw_pointer(fb) {
            stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
        } else {
            write_number(buf, field_val);
        }
    }
    buf.push('}');
    true
}

/// Depth-aware variant of stringify_array for recursive calls.
unsafe fn stringify_array_depth(ptr: *const u8, buf: &mut String, depth: u32) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    // Homogeneous-shape fast path for arrays of objects sharing one
    // `keys_array` (issue #59). The template is built from element 0 and
    // reused for every subsequent element whose shape matches; mismatches
    // fall back per-element via `stringify_value_depth`, so mixed arrays
    // still produce correct output. Pre-check the tag inline to skip the
    // function call entirely for arrays of primitives (issue #64) — common
    // for nested fields like `tags: ["x","y"]` that fired per-element.
    let template = if len >= 2 {
        let first_bits = (*elements).to_bits();
        let tag = first_bits & 0xFFFF_0000_0000_0000;
        if tag == POINTER_TAG || is_raw_pointer(first_bits) {
            build_shape_prefix_template(first_bits)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(ref tmpl) = template {
        buf.push('[');
        for i in 0..len {
            if i > 0 {
                buf.push(',');
            }
            let elem = *elements.add(i as usize);
            let elem_bits = elem.to_bits();
            if !try_emit_shape_element(elem_bits, tmpl, buf, depth) {
                // Match the slow path: array descent does not bump depth.
                stringify_value_depth(elem, TYPE_UNKNOWN, buf, depth);
            }
        }
        buf.push(']');
        return;
    }

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;

        if elem_bits == TAG_UNDEFINED {
            buf.push_str("null");
        } else if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(elem_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
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
        } else if elem_tag == BIGINT_TAG {
            let bigint_ptr = (elem_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_tag == POINTER_TAG || is_raw_pointer(elem_bits) {
            let elem_ptr = if elem_tag == POINTER_TAG {
                (elem_bits & POINTER_MASK) as *const u8
            } else {
                elem_bits as *const u8
            };
            // Issue #639: Buffer/Uint8Array detection BEFORE gc_obj_type — see
            // the matching branch in `stringify_value`.
            if crate::buffer::is_registered_buffer(elem_ptr as usize) {
                stringify_buffer(elem_ptr, buf);
                continue;
            }
            match gc_obj_type(elem_ptr) {
                crate::gc::GC_TYPE_OBJECT => stringify_object_inner(elem_ptr, buf, depth),
                crate::gc::GC_TYPE_ARRAY => stringify_array_depth(elem_ptr, buf, depth),
                crate::gc::GC_TYPE_STRING => {
                    let str_ptr = elem_ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
                _ => {
                    if is_object_pointer(elem_ptr) {
                        stringify_object_inner(elem_ptr, buf, depth);
                    } else {
                        let arr_elem = elem_ptr as *const crate::ArrayHeader;
                        let arr_len = (*arr_elem).length;
                        let arr_cap = (*arr_elem).capacity;
                        if arr_len <= arr_cap && arr_cap > 0 && arr_cap < 10000 {
                            stringify_array_depth(elem_ptr, buf, depth);
                        } else {
                            let str_ptr = elem_ptr as *const StringHeader;
                            if let Some(s) = str_from_header(str_ptr) {
                                write_escaped_string(buf, s);
                            } else {
                                buf.push_str("null");
                            }
                        }
                    }
                }
            }
        } else {
            // Number — or Date, handled centrally by `write_number`
            // via DATE_REGISTRY lookup.
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
            let arr = ptr as *const crate::ArrayHeader;
            let len = (*arr).length as usize;
            return (len * 300).max(256);
        }
        if type_hint == TYPE_OBJECT || is_object_pointer(ptr) {
            let obj = ptr as *const crate::ObjectHeader;
            let fields = (*obj).field_count as usize;
            return (fields * 200).max(256);
        }
    }
    4096
}

/// Generic JSON.stringify that handles any JSValue
/// Takes a f64 (NaN-boxed JSValue) and a type_hint (0=unknown, 1=object, 2=array)
/// Returns a string pointer
#[no_mangle]
/// Issue #179 Step 2 Phase 3: if `value` is a lazy array that's
/// already been materialized (indexed access forced
/// `force_materialize_lazy`), return a JSValue pointing at the
/// materialized `ArrayHeader` tree instead of the `LazyArrayHeader`.
/// The generic tree-walk stringifier would otherwise read lazy-
/// header fields (magic, root_idx, blob_str, ...) as if they were
/// element f64s and crash on the first bogus pointer deref. No-op
/// for non-lazy values and for lazy values whose `materialized` is
/// still null (the lazy-stringify fast path handles those).
unsafe fn redirect_lazy_to_materialized(value: f64) -> f64 {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let ptr = if top16 == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else {
        return value;
    };
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return value;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return value;
    }
    let lazy = ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return value;
    }
    if (*lazy).materialized.is_null() {
        return value;
    }
    f64::from_bits(JSValue::object_ptr((*lazy).materialized as *mut u8).bits())
}

/// Issue #179 Phase 4: lazy-stringify fast path. If `value` is a
/// lazy-parse top-level array whose `materialized` is still null (no
/// indexed access or mutation has forced tree build), memcpy the
/// original blob bytes into a fresh string — no tree walk, no
/// escape handling. Returns `None` if `value` is not a
/// tape-backed-and-unmutated lazy array, in which case the caller
/// falls through to the generic stringify path.
///
/// Correctness invariant: if the lazy value is unmutated, the bytes
/// spanning `[root.offset .. root_end.offset+1]` in the original
/// blob are exactly what `JSON.stringify` would produce for that
/// value (modulo whitespace the user's original blob may contain —
/// `JSON.stringify` never emits whitespace for the 2-arg form, so
/// this is only correct when the blob came from `JSON.stringify` or
/// is otherwise whitespace-free in the array span).
unsafe fn try_stringify_lazy_array(value: f64) -> Option<*mut StringHeader> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let maybe_ptr = if top16 == 0x7FFD {
        // POINTER_TAG NaN-box: lower 48 bits are the user pointer.
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else if top16 == 0 {
        // Raw heap pointer (no NaN-box tag). User-space addresses on
        // 64-bit systems fit in the lower 48 bits, so a real raw
        // pointer has top16 == 0. The previous `top16 < 0x7FF8` check
        // also accepted regular f64 numbers (e.g. 42.0 has top16
        // 0x4045) and `gc_header = bits - 8` then dereferenced random
        // memory, segfaulting `JSON.stringify(42)` at
        // `0x4044_FFFF_FFFF_FFF8`.
        bits as *const u8
    } else {
        return None;
    };
    if maybe_ptr.is_null() || (maybe_ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = maybe_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return None;
    }
    let lazy = maybe_ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC || !(*lazy).materialized.is_null() {
        return None;
    }
    // Phase 5: if the sparse per-element cache has ANY bit set,
    // stringify might miss mutations made through a cached element
    // (e.g. `parsed[0].name = "x"` modifies the materialized object
    // but leaves the blob bytes untouched). Force-materialize the
    // full tree (which consults the sparse cache and preserves
    // cached mutations), then bail out so `redirect_lazy_to_materialized`
    // forwards to the materialized ArrayHeader on the next stringify
    // dispatch. No bits set means we haven't handed any pointers to
    // user code yet, so the blob bytes are authoritative.
    if !(*lazy).materialized_bitmap.is_null() && (*lazy).cached_length > 0 {
        let bitmap = (*lazy).materialized_bitmap;
        let bitmap_words = ((*lazy).cached_length as usize).div_ceil(64);
        let mut has_bits = false;
        for w in 0..bitmap_words {
            if *bitmap.add(w) != 0 {
                has_bits = true;
                break;
            }
        }
        if has_bits {
            crate::json_tape::force_materialize_lazy(
                lazy as *mut crate::json_tape::LazyArrayHeader,
            );
            return None;
        }
    }
    let tape = crate::json_tape::LazyArrayHeader::tape_slice(lazy);
    let blob_bytes = crate::json_tape::LazyArrayHeader::blob_bytes(lazy);
    if tape.is_empty() {
        return None;
    }
    let root = (*lazy).root_idx as usize;
    let start = tape[root].offset as usize;
    let end_idx = tape[root].link as usize;
    let end = tape[end_idx].offset as usize + 1; // +1 includes `]`
    if end > blob_bytes.len() || start > end {
        return None;
    }
    let slice = &blob_bytes[start..end];
    Some(json_string_from_output_bytes(slice))
}

#[no_mangle]
pub unsafe extern "C" fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader {
    if let Some(ptr) = try_stringify_lazy_array(value) {
        return ptr;
    }
    // If the value is a lazy array that's already been materialized
    // (indexed access forced it into a real tree), stringify the
    // tree directly — the generic walker would otherwise read the
    // LazyArrayHeader's fields as if they were array elements and
    // crash on the first deref of a bogus pointer.
    let value = redirect_lazy_to_materialized(value);

    // Non-reentrant fast path (issue #67): skip the shape_cache save/restore
    // round-trip (two RefCell.borrow_mut's + a Vec mem::take/assign) for the
    // common outermost call. A simple Cell-based depth counter identifies
    // reentrant calls (toJSON callbacks); only those pay for the save.
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let mut buf = take_stringify_buf();
    // Scratch buffer is pre-sized to 4096 on first thread-local init and
    // retained across calls, so most small stringifies never hit a
    // String::reserve. `push_str` grows on overflow for the rare
    // single-call output that exceeds that, so skip the estimate call
    // (issue #67: it was ~10ns of wasted work per call for small values).
    stringify_value(value, type_hint, &mut buf);
    let ptr = json_string_from_output_bytes(buf.as_bytes());
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    ptr
}

// ─── Specialized stringify functions ──────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_string(
    str_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match str_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let mut buf = String::with_capacity(s.len() + 16);
    write_escaped_string(&mut buf, s);
    js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

/// Stringify a number
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_number(value: f64) -> *mut StringHeader {
    if value.is_nan() || value.is_infinite() {
        return js_string_from_bytes(b"null".as_ptr(), 4);
    }
    if value.fract() == 0.0 && value.abs() < (i64::MAX as f64) {
        let mut itoa_buf = itoa::Buffer::new();
        let s = itoa_buf.format(value as i64);
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }
    let mut ryu_buf = ryu::Buffer::new();
    let s = ryu_buf.format(value);
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Stringify a boolean
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_bool(value: bool) -> *mut StringHeader {
    let s = if value { "true" } else { "false" };
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Stringify null
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_null() -> *mut StringHeader {
    js_string_from_bytes(b"null".as_ptr(), 4)
}

/// Check if a string is valid JSON
#[no_mangle]
pub unsafe extern "C" fn js_json_is_valid(text_ptr: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if text_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let len = (*text_ptr).byte_len as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    if serde_json::from_slice::<serde_json::Value>(bytes).is_ok() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

// ─── Utility functions ────────────────────────────────────────────────────────

/// Legacy wrapper that allocates a String from a StringHeader
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    str_from_header(ptr).map(|s| s.to_string())
}

/// Get a value from parsed JSON by key (for object access)
#[no_mangle]
pub unsafe extern "C" fn js_json_get_string(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> *mut StringHeader {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return std::ptr::null_mut(),
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        if let Some(serde_json::Value::String(s)) = obj.get(&key) {
            return js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get a number from parsed JSON by key
#[no_mangle]
pub unsafe extern "C" fn js_json_get_number(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> f64 {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return f64::NAN,
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return f64::NAN,
    };
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        if let Some(serde_json::Value::Number(n)) = obj.get(&key) {
            return n.as_f64().unwrap_or(f64::NAN);
        }
    }
    f64::NAN
}

/// Get a boolean from parsed JSON by key
#[no_mangle]
pub unsafe extern "C" fn js_json_get_bool(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> bool {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return false,
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return false,
    };
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        if let Some(serde_json::Value::Bool(b)) = obj.get(&key) {
            return *b;
        }
    }
    false
}

// ─── JSON.stringify with replacer ────────────────────────────────────────────

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Call a replacer closure with (key, value) and return the result as f64
#[inline]
unsafe fn call_replacer(
    replacer: *const crate::ClosureHeader,
    key_f64: f64,
    value_f64: f64,
) -> f64 {
    crate::js_closure_call2(replacer, key_f64, value_f64)
}

/// NaN-box a string pointer as f64 (STRING_TAG)
#[inline]
fn nanbox_string_f64(ptr: *const StringHeader) -> f64 {
    f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK))
}

/// NaN-box an object/array pointer as f64 (POINTER_TAG)
#[inline]
fn nanbox_pointer_f64(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

/// Stringify a value with replacer support.
/// The replacer is called as replacer(key, value) for each property.
/// Returns the replaced value serialized into the buffer.
unsafe fn stringify_value_with_replacer(
    key_f64: f64,
    value: f64,
    type_hint: u32,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    // Call the replacer with (key, value)
    let replaced = call_replacer(replacer, key_f64, value);
    let replaced_bits = replaced.to_bits();

    // If replacer returns undefined, skip this value
    if replaced_bits == TAG_UNDEFINED {
        return;
    }

    // Check if the replaced value is the same as the original (common case)
    // If it is, and the original is an object/array, recurse into it with replacer
    let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;

    // If the replaced value is a string, serialize it as a JSON string
    if replaced_tag == STRING_TAG {
        let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }
    if replaced_tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(replaced_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // If it's null/bool/number, serialize directly
    if replaced_bits == TAG_NULL {
        buf.push_str("null");
        return;
    }
    if replaced_bits == TAG_TRUE {
        buf.push_str("true");
        return;
    }
    if replaced_bits == TAG_FALSE {
        buf.push_str("false");
        return;
    }

    // Check for BigInt tag - serialize as number (toString)
    if replaced_tag == BIGINT_TAG {
        let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            // BigInt toString gives a plain number string, write it directly (no quotes)
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // Check for pointer (object/array) - recurse with replacer
    if let Some(ptr) = extract_pointer(replaced_bits) {
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_with_replacer(ptr, replacer, buf);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_with_replacer(ptr, replacer, buf);
        } else {
            // Try to detect: object vs array
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 && !is_object_pointer(ptr) {
                    stringify_array_with_replacer(ptr, replacer, buf);
                    return;
                }
            }
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, replacer, buf);
            } else {
                // Fallback: serialize as plain value (without replacer)
                stringify_value(replaced, TYPE_UNKNOWN, buf);
            }
        }
        return;
    }

    // Plain number
    write_number(buf, replaced);
}

unsafe fn stringify_object_with_replacer(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    buf.push('{');

    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;

    // Use keys_len as the iteration count since field_count may include pre-allocated slots.
    let actual_fields = std::cmp::min(num_fields, keys_len);
    let mut first = true;
    for f in 0..actual_fields {
        // Get the key as a string
        let (key_str_ptr, key_str_opt) = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let kp = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            (kp, str_from_header(kp))
        } else {
            (std::ptr::null(), None)
        };

        // Create NaN-boxed key for replacer
        let key_f64_for_replacer = if !key_str_ptr.is_null() {
            nanbox_string_f64(key_str_ptr)
        } else {
            // Fallback: create a "fieldN" string
            let fallback = format!("field{}", f);
            let fallback_ptr = js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32);
            nanbox_string_f64(fallback_ptr)
        };

        // Get the field value
        let field_val = *fields_ptr.add(f as usize);

        // Call replacer with (key, value)
        let replaced = call_replacer(replacer, key_f64_for_replacer, field_val);
        let replaced_bits = replaced.to_bits();

        // If replacer returns undefined, skip this property
        if replaced_bits == TAG_UNDEFINED {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

        // Write the key
        if let Some(key_str) = key_str_opt {
            buf.push('"');
            buf.push_str(key_str);
            buf.push_str("\":");
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }

        // Stringify the replaced value
        // For nested objects/arrays, we need to recurse with the replacer
        let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
        if replaced_tag == STRING_TAG {
            let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(replaced_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_bits == TAG_NULL {
            buf.push_str("null");
        } else if replaced_bits == TAG_TRUE {
            buf.push_str("true");
        } else if replaced_bits == TAG_FALSE {
            buf.push_str("false");
        } else if replaced_tag == BIGINT_TAG {
            let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                buf.push_str(s);
            } else {
                buf.push_str("null");
            }
        } else if let Some(inner_ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(inner_ptr) {
                stringify_object_with_replacer(inner_ptr, replacer, buf);
            } else {
                let arr = inner_ptr as *const crate::ArrayHeader;
                if !arr.is_null() {
                    let len = (*arr).length;
                    let cap = (*arr).capacity;
                    if len <= cap && cap > 0 && cap < 10000 {
                        stringify_array_with_replacer(inner_ptr, replacer, buf);
                    } else {
                        stringify_value(replaced, TYPE_UNKNOWN, buf);
                    }
                } else {
                    stringify_value(replaced, TYPE_UNKNOWN, buf);
                }
            }
        } else {
            write_number(buf, replaced);
        }
    }
    buf.push('}');
}

unsafe fn stringify_array_with_replacer(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let elem = *elements.add(i as usize);

        // Create key string for the index
        let idx_str = i.to_string();
        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let key_f64 = nanbox_string_f64(idx_ptr);

        // Call replacer with (index_string, value)
        let replaced = call_replacer(replacer, key_f64, elem);
        let replaced_bits = replaced.to_bits();

        // For arrays, undefined becomes null (per JSON spec)
        if replaced_bits == TAG_UNDEFINED {
            buf.push_str("null");
            continue;
        }

        let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
        if replaced_tag == STRING_TAG {
            let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(replaced_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_bits == TAG_NULL {
            buf.push_str("null");
        } else if replaced_bits == TAG_TRUE {
            buf.push_str("true");
        } else if replaced_bits == TAG_FALSE {
            buf.push_str("false");
        } else if replaced_tag == BIGINT_TAG {
            let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                buf.push_str(s);
            } else {
                buf.push_str("null");
            }
        } else if let Some(inner_ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(inner_ptr) {
                stringify_object_with_replacer(inner_ptr, replacer, buf);
            } else {
                let inner_arr = inner_ptr as *const crate::ArrayHeader;
                if !inner_arr.is_null() {
                    let inner_len = (*inner_arr).length;
                    let inner_cap = (*inner_arr).capacity;
                    if inner_len <= inner_cap && inner_cap > 0 && inner_cap < 10000 {
                        stringify_array_with_replacer(inner_ptr, replacer, buf);
                    } else {
                        stringify_value(replaced, TYPE_UNKNOWN, buf);
                    }
                } else {
                    stringify_value(replaced, TYPE_UNKNOWN, buf);
                }
            }
        } else {
            write_number(buf, replaced);
        }
    }
    buf.push(']');
}

/// JSON.stringify with replacer function
/// value: the JSValue to stringify (NaN-boxed f64)
/// type_hint: 0=unknown, 1=object, 2=array
/// replacer_ptr: pointer to a ClosureHeader (the replacer function)
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_with_replacer(
    value: f64,
    type_hint: u32,
    replacer_ptr: i64,
) -> *mut StringHeader {
    let replacer = replacer_ptr as *const crate::ClosureHeader;
    if replacer.is_null() {
        // Fall back to normal stringify if replacer is null
        return js_json_stringify(value, type_hint);
    }

    // Per JSON spec, the initial call to the replacer is with key="" and the root value
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);

    // Call replacer with ("", root_value)
    let replaced_root = call_replacer(replacer, empty_key_f64, value);
    let replaced_bits = replaced_root.to_bits();

    // If replacer returns undefined for root, return undefined (represented as "undefined" string? No, just return null)
    if replaced_bits == TAG_UNDEFINED {
        return std::ptr::null_mut();
    }

    // Non-reentrant fast path (issue #67): same depth-counter trick as
    // js_json_stringify — skip shape_cache save for the outermost call.
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let estimated = estimate_json_size(value, type_hint);
    let mut buf = take_stringify_buf();
    if buf.capacity() < estimated {
        buf.reserve(estimated - buf.capacity());
    }

    // Check what the replacer returned
    let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
    if replaced_tag == STRING_TAG {
        let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(&mut buf, s);
        } else {
            buf.push_str("null");
        }
    } else if replaced_tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(replaced_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(&mut buf, s);
        } else {
            buf.push_str("null");
        }
    } else if replaced_bits == TAG_NULL {
        buf.push_str("null");
    } else if replaced_bits == TAG_TRUE {
        buf.push_str("true");
    } else if replaced_bits == TAG_FALSE {
        buf.push_str("false");
    } else if replaced_tag == BIGINT_TAG {
        let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
    } else if let Some(ptr) = extract_pointer(replaced_bits) {
        // Object or array - recurse with replacer
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_with_replacer(ptr, replacer, &mut buf);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_with_replacer(ptr, replacer, &mut buf);
        } else {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, replacer, &mut buf);
            } else {
                let arr = ptr as *const crate::ArrayHeader;
                if !arr.is_null() {
                    let len = (*arr).length;
                    let cap = (*arr).capacity;
                    if len <= cap && cap > 0 && cap < 10000 {
                        stringify_array_with_replacer(ptr, replacer, &mut buf);
                    } else {
                        stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                    }
                } else {
                    stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                }
            }
        }
    } else {
        // Number
        write_number(&mut buf, replaced_root);
    }

    let result = js_string_from_bytes(buf.as_ptr(), buf.len() as u32);
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    result
}

// ─── Pretty-print stringify ─────────────────────────────────────────────────

unsafe fn stringify_value_pretty(
    value: f64,
    type_hint: u32,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    let bits: u64 = value.to_bits();

    if bits == TAG_NULL || bits == TAG_UNDEFINED {
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
    // SSO (v0.5.213): decode inline 5-byte string, emit escaped.
    if tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if tag == BIGINT_TAG {
        let bigint_ptr = (bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_pretty(ptr, buf, indent, depth);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_pretty(ptr, buf, indent, depth);
        } else {
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 && !is_object_pointer(ptr) {
                    stringify_array_pretty(ptr, buf, indent, depth);
                    return;
                }
            }
            if is_object_pointer(ptr) {
                stringify_object_pretty(ptr, buf, indent, depth);
            } else {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
        }
        return;
    }

    write_number(buf, value);
}

unsafe fn stringify_object_pretty(ptr: *const u8, buf: &mut String, indent: &str, depth: usize) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    // Check for toJSON method
    if let Some(to_json_val) = object_get_to_json(ptr) {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        stringify_value_pretty(to_json_val, TYPE_UNKNOWN, buf, indent, depth);
        return;
    }

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let actual_fields = std::cmp::min(num_fields, keys_len);

    // Collect non-undefined, non-closure fields
    let mut entries: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let field_bits = field_val.to_bits();
        if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
            continue;
        }
        let key_name = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        entries.push((key_name, field_val));
    }

    if entries.is_empty() {
        buf.push_str("{}");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }

    buf.push_str("{\n");
    let inner_indent_count = depth + 1;
    for (i, (key_name, field_val)) in entries.iter().enumerate() {
        for _ in 0..inner_indent_count {
            buf.push_str(indent);
        }
        buf.push('"');
        buf.push_str(key_name);
        buf.push_str("\": ");
        stringify_value_pretty(*field_val, TYPE_UNKNOWN, buf, indent, inner_indent_count);
        if i + 1 < entries.len() {
            buf.push(',');
        }
        buf.push('\n');
    }
    for _ in 0..depth {
        buf.push_str(indent);
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

unsafe fn stringify_array_pretty(ptr: *const u8, buf: &mut String, indent: &str, depth: usize) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    if len == 0 {
        buf.push_str("[]");
        return;
    }

    buf.push_str("[\n");
    let inner_indent_count = depth + 1;
    for i in 0..len {
        for _ in 0..inner_indent_count {
            buf.push_str(indent);
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        if elem_bits == TAG_UNDEFINED {
            buf.push_str("null");
        } else {
            stringify_value_pretty(elem, TYPE_UNKNOWN, buf, indent, inner_indent_count);
        }
        if i + 1 < len {
            buf.push(',');
        }
        buf.push('\n');
    }
    for _ in 0..depth {
        buf.push_str(indent);
    }
    buf.push(']');
}

// ─── Array replacer (key whitelist) stringify ────────────────────────────────

unsafe fn stringify_object_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let actual_fields = std::cmp::min(num_fields, keys_len);

    // Build a map of key_name -> field_value for the object
    let mut field_map: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let key_name = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        field_map.push((key_name, field_val));
    }

    buf.push('{');
    let mut first = true;
    for allowed_key in allowed_keys {
        if let Some((_, field_val)) = field_map.iter().find(|(k, _)| k == allowed_key) {
            let field_bits = field_val.to_bits();
            if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
                continue;
            }
            if !first {
                buf.push(',');
            }
            first = false;
            if use_pretty {
                buf.push('\n');
                let inner_indent_count = depth + 1;
                for _ in 0..inner_indent_count {
                    buf.push_str(indent);
                }
                buf.push('"');
                buf.push_str(allowed_key);
                buf.push_str("\": ");
                stringify_value_pretty(*field_val, TYPE_UNKNOWN, buf, indent, inner_indent_count);
            } else {
                buf.push('"');
                buf.push_str(allowed_key);
                buf.push_str("\":");
                stringify_value(*field_val, TYPE_UNKNOWN, buf);
            }
        }
    }
    if use_pretty && !first {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

// ─── Extract array of strings from a JSValue array ──────────────────────────

unsafe fn extract_string_array(ptr: *const u8) -> Vec<String> {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut result = Vec::new();
    for i in 0..len {
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;
        if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                result.push(s.to_string());
            }
        } else if elem_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(elem_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                result.push(s.to_string());
            }
        } else if is_raw_pointer(elem_bits) {
            let str_ptr = elem_bits as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                result.push(s.to_string());
            }
        }
    }
    result
}

/// Detect whether a NaN-boxed value is an array (not an object).
#[inline]
unsafe fn is_array_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        if is_object_pointer(ptr) {
            return false;
        }
        let arr = ptr as *const crate::ArrayHeader;
        let len = (*arr).length;
        let cap = (*arr).capacity;
        len <= cap && cap > 0 && cap < 10000
    } else {
        false
    }
}

// ─── Full JSON.stringify(value, replacer, spacer) ───────────────────────────

/// JSON.stringify(value, replacer, spacer) — the full 3-arg form.
///
/// - `value`: NaN-boxed JSValue to stringify
/// - `replacer_f64`: NaN-boxed — a closure (function replacer), array (key whitelist), or null
/// - `spacer_f64`: NaN-boxed — a number (indent count), string (indent string), or null
///
/// Returns i64 JSValue bits: a NaN-boxed string pointer, or TAG_UNDEFINED when
/// `JSON.stringify(undefined)` should return `undefined`.
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_full(
    value: f64,
    replacer_f64: f64,
    spacer_f64: f64,
) -> i64 {
    let value_bits = value.to_bits();

    // JSON.stringify(undefined) returns undefined per spec
    if value_bits == TAG_UNDEFINED {
        return TAG_UNDEFINED as i64;
    }

    // If the value is a closure/function, return undefined per spec
    if is_closure_value(value_bits) {
        return TAG_UNDEFINED as i64;
    }

    // Issue #179 Phase 4: lazy-stringify fast path for unmutated
    // lazy arrays — only when no replacer / no indent (matches the
    // output `JSON.stringify(value)` produces; replacer/indent
    // require a real tree walk). The bench's 2-arg form (and most
    // real usage) hits this path.
    let replacer_bits = replacer_f64.to_bits();
    let spacer_bits = spacer_f64.to_bits();
    let no_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;
    let no_spacer =
        spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE;
    if no_replacer && no_spacer {
        if let Some(ptr) = try_stringify_lazy_array(value) {
            return JSValue::string_ptr(ptr).bits() as i64;
        }
    }
    // Lazy-but-materialized: the fast path's `materialized.is_null()`
    // check above returns None; fall back to the tree walk, but
    // point it at the materialized tree (not the lazy header
    // whose fields aren't element f64s).
    let value = redirect_lazy_to_materialized(value);
    let value_bits = value.to_bits();

    // Determine spacer/indent
    let indent_str: String;
    let spacer_bits = spacer_f64.to_bits();
    let spacer_tag = spacer_bits & 0xFFFF_0000_0000_0000;
    if spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE {
        indent_str = String::new();
    } else if spacer_tag == STRING_TAG {
        let sp_ptr = (spacer_bits & POINTER_MASK) as *const StringHeader;
        indent_str = str_from_header(sp_ptr).unwrap_or("").to_string();
    } else if spacer_tag == crate::value::SHORT_STRING_TAG {
        // v0.5.213 SSO: spacer passed as inline short string
        // (e.g. `JSON.stringify(obj, null, "  ")` where "  " is 2
        // bytes — fits SSO). Decode into scratch, copy into the
        // indent_str buffer for the formatter.
        let jsval = JSValue::from_bits(spacer_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        indent_str = std::str::from_utf8(&scratch[..n]).unwrap_or("").to_string();
    } else if spacer_bits == TAG_TRUE {
        indent_str = String::new();
    } else {
        // Number — use that many spaces (clamped to 10)
        let n = spacer_f64 as usize;
        let n = n.min(10);
        indent_str = " ".repeat(n);
    }
    let use_pretty = !indent_str.is_empty();

    // Determine replacer type
    let replacer_bits = replacer_f64.to_bits();
    let is_null_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;

    // Check if replacer is an array (key whitelist)
    let array_replacer = if !is_null_replacer && is_array_value(replacer_bits) {
        let arr_ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            (replacer_bits & POINTER_MASK) as *const u8
        } else {
            replacer_bits as *const u8
        };
        Some(extract_string_array(arr_ptr))
    } else {
        None
    };

    // Check if replacer is a closure (function)
    let closure_replacer =
        if !is_null_replacer && array_replacer.is_none() && is_closure_value(replacer_bits) {
            let ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
                (replacer_bits & POINTER_MASK) as *const crate::closure::ClosureHeader
            } else {
                replacer_bits as *const crate::closure::ClosureHeader
            };
            Some(ptr)
        } else {
            None
        };

    // Non-reentrant fast path (issue #67): same depth-counter trick as
    // js_json_stringify — skip shape_cache save for the outermost call.
    // Skip the pre-call STRINGIFY_STACK clear: the exit path below always
    // clears it on normal return, and the deep-recursion check at depth
    // > MAX_FAST_DEPTH is robust to leftover entries from a prior panic
    // (a stale ptr that happens to match is a false-positive TypeError,
    // which is a defensible degradation for pathological reentrant cases).
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let mut buf = take_stringify_buf();

    if let Some(ref allowed_keys) = array_replacer {
        // Array replacer: only applies to objects at the top level
        if let Some(ptr) = extract_pointer(value_bits) {
            if is_object_pointer(ptr) {
                stringify_object_with_array_replacer(
                    ptr,
                    allowed_keys,
                    &mut buf,
                    &indent_str,
                    0,
                    use_pretty,
                );
            } else if use_pretty {
                stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
            } else {
                stringify_value(value, TYPE_UNKNOWN, &mut buf);
            }
        } else if use_pretty {
            stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
        } else {
            stringify_value(value, TYPE_UNKNOWN, &mut buf);
        }
    } else if let Some(closure_ptr) = closure_replacer {
        // Function replacer — use existing with_replacer path
        // First call replacer with ("", root_value)
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_f64 = nanbox_string_f64(empty_str);
        let replaced_root = call_replacer(closure_ptr, empty_key_f64, value);
        let replaced_bits = replaced_root.to_bits();
        if replaced_bits == TAG_UNDEFINED {
            STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
            // Restore shape cache and decrement depth before early return
            // (we already incremented STRINGIFY_DEPTH and took the cache).
            restore_stringify_buf(buf);
            match saved_cache {
                Some(s) => restore_shape_cache(s),
                None => clear_shape_cache(),
            }
            STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
            return TAG_UNDEFINED as i64;
        }
        // For simplicity: when function replacer is used with pretty, we don't
        // interleave pretty-printing (matches simple spec behavior). Serialize
        // normally with the replacer.
        if let Some(ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, closure_ptr, &mut buf);
            } else {
                let arr = ptr as *const crate::ArrayHeader;
                if !arr.is_null()
                    && (*arr).length <= (*arr).capacity
                    && (*arr).capacity > 0
                    && (*arr).capacity < 10000
                {
                    stringify_array_with_replacer(ptr, closure_ptr, &mut buf);
                } else {
                    stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                }
            }
        } else {
            stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
        }
    } else if use_pretty {
        // No replacer, but has spacer — pretty-print
        stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
    } else {
        // Plain stringify
        stringify_value(value, TYPE_UNKNOWN, &mut buf);
    }

    // Only touch STRINGIFY_STACK if we actually pushed to it (depth >
    // MAX_FAST_DEPTH was hit). The `borrow` path avoids the borrow_mut
    // cost on the common empty-stack case. Unpopped entries only exist
    // after a panic mid-traversal; see the entry-side comment for the
    // correctness argument.
    STRINGIFY_STACK.with(|s| {
        let stack = s.borrow();
        if !stack.is_empty() {
            drop(stack);
            s.borrow_mut().clear();
        }
    });

    let result_ptr = json_string_from_output_bytes(buf.as_bytes());
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    // Return as NaN-boxed string
    (STRING_TAG | (result_ptr as u64 & POINTER_MASK)) as i64
}

// ─── JSON.parse with reviver ────────────────────────────────────────────────

/// Apply reviver to a parsed JSON value. The reviver is called as reviver(key, value).
/// For objects, it's called for each property; for the root, key is "".
unsafe fn apply_reviver(
    value: JSValue,
    key_f64: f64,
    reviver: *const crate::closure::ClosureHeader,
) -> JSValue {
    let bits = value.bits();

    // If value is an object, recurse into its properties first
    if let Some(ptr) = extract_pointer(bits) {
        if is_object_pointer(ptr) {
            let obj = ptr as *const crate::ObjectHeader;
            let num_fields = (*obj).field_count;
            let keys_arr = (*obj).keys_array;
            let keys_len = (*keys_arr).length;
            let keys_elements = (keys_arr as *const u8)
                .add(std::mem::size_of::<crate::ArrayHeader>())
                as *const f64;
            let fields_ptr =
                (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut f64;
            let actual_fields = std::cmp::min(num_fields, keys_len);

            for f in 0..actual_fields {
                let field_key_f64 = *keys_elements.add(f as usize);
                let field_val_f64 = *fields_ptr.add(f as usize);
                let child_val = JSValue::from_bits(field_val_f64.to_bits());
                let revived_child = apply_reviver(child_val, field_key_f64, reviver);
                // Write back the revived value
                *fields_ptr.add(f as usize) = f64::from_bits(revived_child.bits());
                crate::gc::layout_note_slot(obj as usize, f as usize, revived_child.bits());
            }
        } else {
            // Check if it's an array
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 {
                    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>())
                        as *mut f64;
                    for i in 0..len {
                        let idx_str = i.to_string();
                        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
                        let idx_key_f64 = nanbox_string_f64(idx_ptr);
                        let elem_f64 = *elements.add(i as usize);
                        let child_val = JSValue::from_bits(elem_f64.to_bits());
                        let revived_child = apply_reviver(child_val, idx_key_f64, reviver);
                        *elements.add(i as usize) = f64::from_bits(revived_child.bits());
                        crate::array::note_array_slot(
                            arr as *mut crate::ArrayHeader,
                            i as usize,
                            revived_child.bits(),
                        );
                    }
                }
            }
        }
    }

    // Now call reviver on this value
    let value_f64 = f64::from_bits(value.bits());
    let result = crate::js_closure_call2(reviver, key_f64, value_f64);
    JSValue::from_bits(result.to_bits())
}

/// JSON.parse(text, reviver) — parse JSON with a reviver function.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_with_reviver(
    text_ptr: *const StringHeader,
    reviver_ptr: i64,
) -> JSValue {
    // First, parse normally
    let parsed = js_json_parse(text_ptr);

    let reviver = reviver_ptr as *const crate::closure::ClosureHeader;
    if reviver.is_null() || (reviver_ptr as u64) < 0x1000 {
        return parsed;
    }

    // Apply reviver starting from root
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);
    apply_reviver(parsed, empty_key_f64, reviver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_parse_shared_shape_preserves_duplicate_key_semantics() {
        let input = br#"{"a":1,"a":2,"b":{"c":3}}"#;
        let text = js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let value = unsafe { js_json_parse(text) };
        let obj = (value.bits() & POINTER_MASK) as *const crate::ObjectHeader;
        let key_a = js_string_from_bytes(b"a".as_ptr(), 1);
        let key_b = js_string_from_bytes(b"b".as_ptr(), 1);

        unsafe {
            assert_eq!((*(*obj).keys_array).length, 2);
        }
        let a = crate::object::js_object_get_field_by_name(obj, key_a);
        assert_eq!(f64::from_bits(a.bits()), 2.0);

        let b = crate::object::js_object_get_field_by_name(obj, key_b);
        assert!(b.is_pointer());
        let nested = (b.bits() & POINTER_MASK) as *const crate::ObjectHeader;
        unsafe {
            assert_eq!((*(*nested).keys_array).length, 1);
        }
    }

    #[test]
    fn direct_parse_shared_shape_roundtrips_array_records() {
        let input = br#"[{"id":1,"name":"item_1","nested":{"x":1,"y":2}},{"id":2,"name":"item_2","nested":{"x":2,"y":4}}]"#;
        let text = js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let value = unsafe { js_json_parse(text) };
        let output = unsafe { js_json_stringify(f64::from_bits(value.bits()), TYPE_UNKNOWN) };

        assert_eq!(
            unsafe { str_from_header(output).unwrap() },
            std::str::from_utf8(input).unwrap()
        );
    }

    #[test]
    fn stringify_non_ascii_output_keeps_utf16_len_distinct_from_bytes() {
        let input = crate::string::js_string_from_bytes("é".as_ptr(), "é".len() as u32);
        let value = f64::from_bits(crate::value::JSValue::string_ptr(input).bits());
        let output = unsafe { js_json_stringify(value, TYPE_UNKNOWN) };

        assert_eq!(unsafe { str_from_header(output).unwrap() }, "\"é\"");
        assert_eq!(unsafe { (*output).byte_len }, 4);
        assert_eq!(unsafe { (*output).utf16_len }, 3);
        assert_eq!(unsafe { (*output).flags }, 0);
    }
}
