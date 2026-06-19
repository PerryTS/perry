//! Issue #179 Step 2 Phase 1: tape representation for JSON values.
//!
//! A *tape* is a flat `Vec<TapeEntry>` recording the structural
//! positions of every significant token in a JSON blob: object/array
//! starts and ends, object key positions, and scalar value positions.
//! Each entry carries a byte-offset into the original blob and a
//! lightweight kind tag. Parsing a JSON document to a tape is a
//! single pass with bounded memory (tape size is O(token count),
//! not O(tree size) — closer to 8-16 bytes per token versus the
//! ~80+ bytes per JSValue object the tree representation costs).
//!
//! This module is the foundation for:
//!   Phase 2 — `JSON.parse(x).length` reads tape's top-level array
//!     length directly, no tree materialization
//!   Phase 3 — indexed/property access on a tape-backed value
//!     materializes only the touched subtree
//!   Phase 4 — `JSON.stringify(taped)` on an unmutated tape memcpys
//!     the original blob bytes instead of walking a tree
//!
//! This Phase 1 commit ships the tape builder + a materializer that
//! produces the same `JSValue` tree as the existing `DirectParser`.
//! It is opt-in via the `PERRY_JSON_TAPE=1` env var so production
//! behavior is unchanged. Correctness is verified by running all
//! existing `JSON.parse` tests through both the direct and
//! tape-materialize paths and comparing their `JSON.stringify`
//! output byte-for-byte.
//!
//! The tape+materialize path intentionally performs no better than
//! the direct path (it does strictly more work). The value lands
//! when Phase 2+ intercept access and skip materialization.

use crate::value::JSValue;
use std::cell::Cell;

/// One tape entry. Kind + byte offset + (for container kinds) a
/// parent/sibling pointer that lets materialization skip over
/// already-traversed subtrees.
#[derive(Debug, Clone, Copy)]
pub struct TapeEntry {
    /// Byte offset into the source blob where this token starts.
    pub offset: u32,
    /// One of the `KIND_*` constants.
    pub kind: u8,
    /// For container kinds (`KIND_OBJ_START` / `KIND_ARR_START`): the
    /// tape index of the matching end marker. Enables O(1) skip-over
    /// during lazy subtree materialization. Zero for leaf kinds.
    pub link: u32,
}

// Tape kinds. 8 bits; ample room for extension (lazy sentinel, hole,
// etc. can be added without widening the struct).
pub const KIND_OBJ_START: u8 = 1;
pub const KIND_OBJ_END: u8 = 2;
pub const KIND_ARR_START: u8 = 3;
pub const KIND_ARR_END: u8 = 4;
pub const KIND_KEY: u8 = 5;
pub const KIND_STRING: u8 = 6;
pub const KIND_NUMBER: u8 = 7;
pub const KIND_TRUE: u8 = 8;
pub const KIND_FALSE: u8 = 9;
pub const KIND_NULL: u8 = 10;

/// The built tape for one JSON document.
pub struct Tape {
    pub entries: Vec<TapeEntry>,
}

struct TapeScratch {
    entries: Vec<TapeEntry>,
    stack: Vec<u32>,
}

impl TapeScratch {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            stack: Vec::new(),
        }
    }

    fn trim_for_reuse(&mut self) {
        self.entries.clear();
        self.stack.clear();

        if self.entries.capacity() * std::mem::size_of::<TapeEntry>()
            > MAX_RETAINED_TAPE_SCRATCH_BYTES
        {
            self.entries = Vec::new();
        }
        if self.stack.capacity() * std::mem::size_of::<u32>() > MAX_RETAINED_TAPE_STACK_BYTES {
            self.stack = Vec::new();
        }
    }
}

const MAX_RETAINED_TAPE_SCRATCH_BYTES: usize = 1024 * 1024;
const MAX_RETAINED_TAPE_STACK_BYTES: usize = 64 * 1024;

thread_local! {
    static TAPE_SCRATCH: Cell<Option<TapeScratch>> = Cell::new(Some(TapeScratch::new()));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum JsonTapeSafepoint {
    MaterializeObjectRooted,
    MaterializeArrayRooted,
    LazyArrayRooted,
    LazyGetHeaderRooted,
    ForceLazyHeaderRooted,
    ForceLazyArrayRooted,
}

#[cfg(test)]
pub type JsonTapeSafepointHook = fn(JsonTapeSafepoint, usize);

#[cfg(test)]
thread_local! {
    static JSON_TAPE_SAFEPOINT_HOOK: Cell<Option<JsonTapeSafepointHook>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn test_set_safepoint_hook(
    hook: Option<JsonTapeSafepointHook>,
) -> Option<JsonTapeSafepointHook> {
    JSON_TAPE_SAFEPOINT_HOOK.with(|slot| {
        let previous = slot.get();
        slot.set(hook);
        previous
    })
}

#[cfg(test)]
#[inline]
fn json_tape_safepoint(point: JsonTapeSafepoint, ptr: usize) {
    JSON_TAPE_SAFEPOINT_HOOK.with(|slot| {
        if let Some(hook) = slot.get() {
            hook(point, ptr);
        }
    });
}

#[cfg(not(test))]
#[inline]
fn json_tape_safepoint(_point: JsonTapeSafepoint, _ptr: usize) {}

/// Build a tape from JSON bytes in one pass. Returns `None` on
/// malformed input (caller should fall through to the direct parser
/// which has richer error reporting).
///
/// The builder walks the input left-to-right and pushes tape entries
/// for every structural token. It does NOT decode strings or numbers
/// — those are deferred to materialization, which lets the tape build
/// pass be byte-scan-only (SIMD-friendly in future revisions) and
/// avoids allocating for values that lazy access will never read.
pub fn build_tape(bytes: &[u8]) -> Option<Tape> {
    let mut entries: Vec<TapeEntry> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    if build_tape_into(bytes, &mut entries, &mut stack) {
        Some(Tape { entries })
    } else {
        None
    }
}

/// Build a tape into caller-provided storage. This is the hot-path
/// variant used by `JSON.parse` so repeated parse-churn workloads do
/// not allocate and free a fresh tape vector on every iteration.
fn build_tape_into(bytes: &[u8], entries: &mut Vec<TapeEntry>, stack: &mut Vec<u32>) -> bool {
    entries.clear();
    stack.clear();
    // Pre-size: worst case is one tape entry per ~4 bytes of input
    // (single-digit integers in an array), though typical JSON is
    // closer to one per 15-20 bytes. Pre-allocating to len/8 is a
    // reasonable middle.
    entries.reserve(bytes.len() / 8 + 8);
    // Parallel stack of (tape index of the matching OBJ/ARR start).
    // On end-of-container, we pop and backfill the start entry's
    // `link` field with the end's tape index.
    let mut pos = 0usize;

    // Helper: skip whitespace.
    #[inline(always)]
    fn skip_ws(bytes: &[u8], pos: &mut usize) {
        while *pos < bytes.len() {
            match bytes[*pos] {
                b' ' | b'\t' | b'\n' | b'\r' => *pos += 1,
                _ => break,
            }
        }
    }

    // Helper: skip a JSON string in place (past the closing quote).
    // Returns `true` on success, `false` on EOF before closing quote.
    // Honors `\"`, `\\`, and other escapes by swallowing the character
    // after a backslash. Does NOT decode — just finds the boundary.
    #[inline(always)]
    fn skip_string(bytes: &[u8], pos: &mut usize) -> bool {
        debug_assert_eq!(bytes[*pos], b'"');
        *pos += 1;
        while *pos < bytes.len() {
            let c = bytes[*pos];
            if c == b'"' {
                *pos += 1;
                return true;
            }
            if c == b'\\' {
                *pos += 1;
                if *pos >= bytes.len() {
                    return false;
                }
                *pos += 1;
            } else {
                *pos += 1;
            }
        }
        false
    }

    // Helper: skip a JSON number (past its last digit/exponent).
    #[inline(always)]
    fn skip_number(bytes: &[u8], pos: &mut usize) {
        if *pos < bytes.len() && bytes[*pos] == b'-' {
            *pos += 1;
        }
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if *pos < bytes.len() && bytes[*pos] == b'.' {
            *pos += 1;
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
        }
        if *pos < bytes.len() && (bytes[*pos] == b'e' || bytes[*pos] == b'E') {
            *pos += 1;
            if *pos < bytes.len() && (bytes[*pos] == b'+' || bytes[*pos] == b'-') {
                *pos += 1;
            }
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
        }
    }

    // Driver: expecting-value state. After emitting a value, the
    // caller handles the trailing `,` or container end.
    enum State {
        Value,
        AfterValue,
    }
    let mut state = State::Value;

    loop {
        skip_ws(bytes, &mut pos);
        if pos >= bytes.len() {
            break;
        }
        match state {
            State::Value => {
                let tok_off = pos as u32;
                match bytes[pos] {
                    b'{' => {
                        let idx = entries.len() as u32;
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_OBJ_START,
                            link: 0,
                        });
                        stack.push(idx);
                        pos += 1;
                        skip_ws(bytes, &mut pos);
                        if pos < bytes.len() && bytes[pos] == b'}' {
                            let end_idx = entries.len() as u32;
                            entries.push(TapeEntry {
                                offset: pos as u32,
                                kind: KIND_OBJ_END,
                                link: idx,
                            });
                            entries[idx as usize].link = end_idx;
                            stack.pop();
                            pos += 1;
                            state = State::AfterValue;
                        } else {
                            // Expect "key":value,...
                            // Handled by the AfterStart branch below.
                            state = State::Value;
                            // Immediately parse the key.
                            if pos >= bytes.len() || bytes[pos] != b'"' {
                                return false;
                            }
                            let key_off = pos as u32;
                            if !skip_string(bytes, &mut pos) {
                                return false;
                            }
                            entries.push(TapeEntry {
                                offset: key_off,
                                kind: KIND_KEY,
                                link: 0,
                            });
                            skip_ws(bytes, &mut pos);
                            if pos >= bytes.len() || bytes[pos] != b':' {
                                return false;
                            }
                            pos += 1;
                        }
                    }
                    b'[' => {
                        let idx = entries.len() as u32;
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_ARR_START,
                            link: 0,
                        });
                        stack.push(idx);
                        pos += 1;
                        skip_ws(bytes, &mut pos);
                        if pos < bytes.len() && bytes[pos] == b']' {
                            let end_idx = entries.len() as u32;
                            entries.push(TapeEntry {
                                offset: pos as u32,
                                kind: KIND_ARR_END,
                                link: idx,
                            });
                            entries[idx as usize].link = end_idx;
                            stack.pop();
                            pos += 1;
                            state = State::AfterValue;
                        } else {
                            state = State::Value;
                        }
                    }
                    b'"' => {
                        if !skip_string(bytes, &mut pos) {
                            return false;
                        }
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_STRING,
                            link: 0,
                        });
                        state = State::AfterValue;
                    }
                    b't' => {
                        if pos + 4 > bytes.len() || &bytes[pos..pos + 4] != b"true" {
                            return false;
                        }
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_TRUE,
                            link: 0,
                        });
                        pos += 4;
                        state = State::AfterValue;
                    }
                    b'f' => {
                        if pos + 5 > bytes.len() || &bytes[pos..pos + 5] != b"false" {
                            return false;
                        }
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_FALSE,
                            link: 0,
                        });
                        pos += 5;
                        state = State::AfterValue;
                    }
                    b'n' => {
                        if pos + 4 > bytes.len() || &bytes[pos..pos + 4] != b"null" {
                            return false;
                        }
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_NULL,
                            link: 0,
                        });
                        pos += 4;
                        state = State::AfterValue;
                    }
                    c if c == b'-' || c.is_ascii_digit() => {
                        skip_number(bytes, &mut pos);
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_NUMBER,
                            link: 0,
                        });
                        state = State::AfterValue;
                    }
                    _ => return false,
                }
            }
            State::AfterValue => {
                if stack.is_empty() {
                    // Top-level value consumed; trailing whitespace is OK.
                    break;
                }
                // Look at which container we're in.
                let top_idx = *stack.last().unwrap();
                let top_kind = entries[top_idx as usize].kind;
                match bytes[pos] {
                    b',' => {
                        pos += 1;
                        if top_kind == KIND_OBJ_START {
                            // Expect next key.
                            skip_ws(bytes, &mut pos);
                            if pos >= bytes.len() || bytes[pos] != b'"' {
                                return false;
                            }
                            let key_off = pos as u32;
                            if !skip_string(bytes, &mut pos) {
                                return false;
                            }
                            entries.push(TapeEntry {
                                offset: key_off,
                                kind: KIND_KEY,
                                link: 0,
                            });
                            skip_ws(bytes, &mut pos);
                            if pos >= bytes.len() || bytes[pos] != b':' {
                                return false;
                            }
                            pos += 1;
                        }
                        state = State::Value;
                    }
                    b'}' if top_kind == KIND_OBJ_START => {
                        let end_idx = entries.len() as u32;
                        entries.push(TapeEntry {
                            offset: pos as u32,
                            kind: KIND_OBJ_END,
                            link: top_idx,
                        });
                        entries[top_idx as usize].link = end_idx;
                        stack.pop();
                        pos += 1;
                        state = State::AfterValue;
                    }
                    b']' if top_kind == KIND_ARR_START => {
                        let end_idx = entries.len() as u32;
                        entries.push(TapeEntry {
                            offset: pos as u32,
                            kind: KIND_ARR_END,
                            link: top_idx,
                        });
                        entries[top_idx as usize].link = end_idx;
                        stack.pop();
                        pos += 1;
                        state = State::AfterValue;
                    }
                    _ => return false,
                }
            }
        }
    }

    if !stack.is_empty() {
        return false;
    } // unclosed container
    if entries.is_empty() {
        return false;
    }
    true
}

/// Build a tape using thread-local scratch storage, then borrow the
/// completed entries for a caller-provided operation. Scratch is kept
/// only while it remains modest; large blobs are allowed to return
/// their backing allocation to the system allocator instead of pinning
/// a high-water tape buffer for the rest of the thread.
pub(crate) fn with_built_tape<R>(bytes: &[u8], f: impl FnOnce(&[TapeEntry]) -> R) -> Option<R> {
    TAPE_SCRATCH.with(|cell| {
        let mut scratch = cell.take().unwrap_or_else(TapeScratch::new);
        let result = if build_tape_into(bytes, &mut scratch.entries, &mut scratch.stack) {
            Some(f(&scratch.entries))
        } else {
            None
        };
        scratch.trim_for_reuse();
        cell.set(Some(scratch));
        result
    })
}

/// Materialize a tape into a `JSValue` tree identical to what the
/// direct parser would produce. Walks the tape from index 0 (the
/// root value) and recursively builds the tree.
///
/// Uses the same runtime allocators as `DirectParser` so the result
/// is GC-tracked + shape-cached identically. The materializer does
/// NOT use the typed-parse shape hint (that's Step 1b's path) —
/// it's the lazy-parse dual: correctness-preserving and order-
/// agnostic.
///
/// Returns `JSValue::null()` on empty tape (caller shouldn't invoke
/// materialize on None tapes, but this keeps the function total).
pub unsafe fn materialize(tape: &Tape, bytes: &[u8]) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = TapeSource::Borrowed {
        tape: &tape.entries,
        bytes,
    };
    let mut idx: usize = 0;
    materialize_value_source(&source, &scope, &mut idx)
}

enum TapeSource<'a, 'scope> {
    Borrowed {
        tape: &'a [TapeEntry],
        bytes: &'a [u8],
    },
    Lazy {
        hdr_handle: crate::gc::RuntimeHandle<'scope>,
    },
}

impl<'a, 'scope> TapeSource<'a, 'scope> {
    #[inline]
    unsafe fn len(&self) -> usize {
        match self {
            TapeSource::Borrowed { tape, .. } => tape.len(),
            TapeSource::Lazy { hdr_handle } => {
                let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
                if hdr.is_null() {
                    0
                } else {
                    (*hdr).tape_len as usize
                }
            }
        }
    }

    #[inline]
    unsafe fn entry(&self, idx: usize) -> Option<TapeEntry> {
        match self {
            TapeSource::Borrowed { tape, .. } => tape.get(idx).copied(),
            TapeSource::Lazy { hdr_handle } => {
                let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
                if hdr.is_null() || idx >= (*hdr).tape_len as usize {
                    return None;
                }
                let base = (hdr as *const u8).add(std::mem::size_of::<LazyArrayHeader>())
                    as *const TapeEntry;
                Some(*base.add(idx))
            }
        }
    }

    #[inline]
    unsafe fn bytes_from_offset(&self, offset: usize) -> &[u8] {
        match self {
            TapeSource::Borrowed { bytes, .. } => {
                if offset <= bytes.len() {
                    &bytes[offset..]
                } else {
                    &[]
                }
            }
            TapeSource::Lazy { hdr_handle } => {
                let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
                if hdr.is_null() {
                    return &[];
                }
                let bytes = LazyArrayHeader::blob_bytes(hdr);
                if offset <= bytes.len() {
                    &bytes[offset..]
                } else {
                    &[]
                }
            }
        }
    }

    #[inline]
    fn is_lazy(&self) -> bool {
        matches!(self, TapeSource::Lazy { .. })
    }
}

/// Source-backed recursive materializer. The borrowed variant is used
/// by eager tape materialization; the lazy variant re-reads tape/blob
/// pointers through a refreshed `LazyArrayHeader` handle instead of
/// carrying slices across safepoints.
#[inline]
unsafe fn materialize_value_source(
    source: &TapeSource<'_, '_>,
    scope: &crate::gc::RuntimeHandleScope,
    idx: &mut usize,
) -> JSValue {
    if *idx >= source.len() {
        return JSValue::null();
    }
    let Some(entry) = source.entry(*idx) else {
        return JSValue::null();
    };
    match entry.kind {
        KIND_OBJ_START => {
            let end_idx = entry.link as usize;
            *idx += 1;
            materialize_object(source, scope, idx, end_idx)
        }
        KIND_ARR_START => {
            let end_idx = entry.link as usize;
            *idx += 1;
            materialize_array(source, scope, idx, end_idx)
        }
        KIND_STRING => {
            *idx += 1;
            materialize_string_value(source, entry.offset as usize)
        }
        KIND_NUMBER => {
            *idx += 1;
            materialize_number(source, entry.offset as usize)
        }
        KIND_TRUE => {
            *idx += 1;
            JSValue::bool(true)
        }
        KIND_FALSE => {
            *idx += 1;
            JSValue::bool(false)
        }
        KIND_NULL => {
            *idx += 1;
            JSValue::null()
        }
        _ => JSValue::null(),
    }
}

unsafe fn materialize_object(
    source: &TapeSource<'_, '_>,
    scope: &crate::gc::RuntimeHandleScope,
    idx: &mut usize,
    end_idx: usize,
) -> JSValue {
    let obj = crate::object::js_object_alloc(0, 0);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    json_tape_safepoint(JsonTapeSafepoint::MaterializeObjectRooted, obj as usize);
    while *idx < end_idx {
        let Some(key_entry) = source.entry(*idx) else {
            break;
        };
        debug_assert_eq!(key_entry.kind, KIND_KEY);
        *idx += 1;
        let key_ptr = decode_key_to_interned_string(source, key_entry.offset as usize);
        let field_scope = crate::gc::RuntimeHandleScope::new();
        let key_handle = field_scope.root_string_ptr(key_ptr);
        let value = materialize_value_source(source, &field_scope, idx);
        let value_handle = field_scope.root_nanbox_u64(value.bits());
        let key_ptr =
            key_handle.get_raw_const_ptr::<crate::StringHeader>() as *mut crate::StringHeader;
        if !key_ptr.is_null() {
            let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            crate::object::js_object_set_field_by_name(
                obj,
                key_ptr,
                f64::from_bits(value_handle.get_nanbox_u64()),
            );
        }
    }
    *idx = end_idx + 1;
    let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
    JSValue::object_ptr(obj as *mut u8)
}

unsafe fn materialize_array(
    source: &TapeSource<'_, '_>,
    scope: &crate::gc::RuntimeHandleScope,
    idx: &mut usize,
    end_idx: usize,
) -> JSValue {
    let arr = crate::array::js_array_alloc(16);
    let arr_handle = scope.root_nanbox_u64(JSValue::object_ptr(arr as *mut u8).bits());
    json_tape_safepoint(JsonTapeSafepoint::MaterializeArrayRooted, arr as usize);
    while *idx < end_idx {
        let elem_scope = crate::gc::RuntimeHandleScope::new();
        let value = materialize_value_source(source, &elem_scope, idx);
        let value_handle = elem_scope.root_nanbox_u64(value.bits());
        let arr = array_from_nanbox_handle(&arr_handle);
        let arr =
            crate::array::js_array_push(arr, JSValue::from_bits(value_handle.get_nanbox_u64()));
        arr_handle.set_nanbox_u64(JSValue::object_ptr(arr as *mut u8).bits());
    }
    *idx = end_idx + 1;
    let arr = array_from_nanbox_handle(&arr_handle);
    JSValue::object_ptr(arr as *mut u8)
}

#[inline]
fn array_from_nanbox_handle(
    handle: &crate::gc::RuntimeHandle<'_>,
) -> *mut crate::array::ArrayHeader {
    (handle.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut crate::array::ArrayHeader
}

/// Decode the string literal starting at `offset` (the opening `"`)
/// into an interned `*mut StringHeader`. Uses the existing
/// `PARSE_KEY_CACHE` (longlived-arena interning) so that repeated
/// records with the same field names share one allocation per key —
/// without this, a 10k-record × 5-key parse materializes 50k fresh
/// longlived strings and the tape path ends up ~3× slower than the
/// direct parser which always went through the cache (`json.rs:448`
/// keyed path in `DirectParser::parse_object`).
unsafe fn decode_key_to_interned_string(
    source: &TapeSource<'_, '_>,
    offset: usize,
) -> *mut crate::StringHeader {
    let bytes_at_key = source.bytes_from_offset(offset);
    let key_bytes: Vec<u8> = match parse_string_bytes_static(bytes_at_key) {
        Some(ParsedStr::Borrowed(slice)) => {
            let cached = crate::json::PARSE_KEY_CACHE.with(|c| c.borrow().get(slice).copied());
            if let Some(p) = cached {
                return p as *mut crate::StringHeader;
            }
            if source.is_lazy() {
                let owned = slice.to_vec();
                let p = crate::string::js_string_from_bytes_longlived(
                    owned.as_ptr(),
                    owned.len() as u32,
                );
                crate::json::PARSE_KEY_CACHE.with(|c| {
                    c.borrow_mut().insert(owned, p);
                });
                return p;
            }
            let p =
                crate::string::js_string_from_bytes_longlived(slice.as_ptr(), slice.len() as u32);
            crate::json::PARSE_KEY_CACHE.with(|c| {
                c.borrow_mut().insert(slice.to_vec(), p);
            });
            return p;
        }
        Some(ParsedStr::Owned(v)) => v,
        None => return std::ptr::null_mut(),
    };
    // Two-phase lookup: check cache with immutable borrow first, then
    // allocate OUTSIDE the borrow (allocation may trigger GC →
    // `scan_parse_roots` → borrow() on same RefCell).
    let cached =
        crate::json::PARSE_KEY_CACHE.with(|c| c.borrow().get(key_bytes.as_slice()).copied());
    if let Some(p) = cached {
        return p as *mut crate::StringHeader;
    }
    let p =
        crate::string::js_string_from_bytes_longlived(key_bytes.as_ptr(), key_bytes.len() as u32);
    crate::json::PARSE_KEY_CACHE.with(|c| {
        c.borrow_mut().insert(key_bytes, p);
    });
    p
}

unsafe fn materialize_string_value(source: &TapeSource<'_, '_>, offset: usize) -> JSValue {
    let bytes_at_val = source.bytes_from_offset(offset);
    match parse_string_bytes_static(bytes_at_val) {
        Some(ParsedStr::Borrowed(slice)) => {
            // v0.5.216 SSO: short-string values inline into the
            // NaN-box payload, zero heap allocation. Only fires
            // when consumers (stringify, equality, length, property
            // access) can handle both forms — Step 1 + 1.5 of the
            // SSO migration landed those consumer arms in v0.5.214
            // / v0.5.215.
            if let Some(sso) = JSValue::try_short_string(slice) {
                return sso;
            }
            let ptr = if source.is_lazy() {
                let owned = slice.to_vec();
                crate::string::js_string_from_bytes(owned.as_ptr(), owned.len() as u32)
            } else {
                crate::string::js_string_from_bytes(slice.as_ptr(), slice.len() as u32)
            };
            JSValue::string_ptr(ptr)
        }
        Some(ParsedStr::Owned(vec)) => {
            if let Some(sso) = JSValue::try_short_string(&vec) {
                return sso;
            }
            let ptr = crate::string::js_string_from_bytes(vec.as_ptr(), vec.len() as u32);
            JSValue::string_ptr(ptr)
        }
        None => JSValue::null(),
    }
}

unsafe fn materialize_number(source: &TapeSource<'_, '_>, offset: usize) -> JSValue {
    // Find the number's end using the same rules as skip_number in
    // the tape builder. Slice then parse.
    let bytes = source.bytes_from_offset(offset);
    let mut end = 0usize;
    if end < bytes.len() && bytes[end] == b'-' {
        end += 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        end += 1;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    let num_str = std::str::from_utf8_unchecked(&bytes[..end]);
    let value: f64 = num_str.parse().unwrap_or(0.0);
    JSValue::number(value)
}

/// Parsed string slot: zero-copy borrow when no escapes, owned when
/// escapes required decoding. Mirrors `DirectParser::ParsedStr`.
enum ParsedStr<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

/// Parse a `"…"` literal starting at `bytes[0]` (the opening quote).
/// Standalone because the materializer doesn't have a live
/// `DirectParser` instance. Same semantics as
/// `DirectParser::parse_string_bytes`.
fn parse_string_bytes_static(bytes: &[u8]) -> Option<ParsedStr<'_>> {
    if bytes.is_empty() || bytes[0] != b'"' {
        return None;
    }
    let mut pos = 1usize;
    let start = pos;
    while pos < bytes.len() {
        let c = bytes[pos];
        if c == b'"' {
            return Some(ParsedStr::Borrowed(&bytes[start..pos]));
        }
        if c == b'\\' {
            // Fall through to slow path from here.
            return parse_string_bytes_slow(bytes, pos, start);
        }
        pos += 1;
    }
    None
}

fn parse_string_bytes_slow(bytes: &[u8], start_pos: usize, start: usize) -> Option<ParsedStr<'_>> {
    let mut result: Vec<u8> = Vec::from(&bytes[start..start_pos]);
    let mut pos = start_pos;
    loop {
        if pos >= bytes.len() {
            return None;
        }
        let c = bytes[pos];
        pos += 1;
        match c {
            b'"' => return Some(ParsedStr::Owned(result)),
            b'\\' => {
                if pos >= bytes.len() {
                    return None;
                }
                let esc = bytes[pos];
                pos += 1;
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
                        if pos + 4 > bytes.len() {
                            return None;
                        }
                        let hex = std::str::from_utf8(&bytes[pos..pos + 4]).ok()?;
                        let code = u16::from_str_radix(hex, 16).ok()?;
                        pos += 4;
                        if (0xD800..=0xDBFF).contains(&code) {
                            if pos + 6 <= bytes.len()
                                && bytes[pos] == b'\\'
                                && bytes[pos + 1] == b'u'
                            {
                                let hex2 = std::str::from_utf8(&bytes[pos + 2..pos + 6]).ok()?;
                                let low = u16::from_str_radix(hex2, 16).ok()?;
                                pos += 6;
                                let codepoint = 0x10000
                                    + ((code as u32 - 0xD800) << 10)
                                    + (low as u32 - 0xDC00);
                                if let Some(ch) = char::from_u32(codepoint) {
                                    let mut buf = [0u8; 4];
                                    let s = ch.encode_utf8(&mut buf);
                                    result.extend_from_slice(s.as_bytes());
                                }
                            }
                        } else if let Some(ch) = char::from_u32(code as u32) {
                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            result.extend_from_slice(s.as_bytes());
                        }
                    }
                    _ => result.push(esc),
                }
            }
            _ => result.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tape structure invariants on a simple object — exercises the
    /// OBJ_START → KEY → scalar → OBJ_END chain and the backfilled
    /// `link` for skip-over.
    #[test]
    fn tape_simple_object() {
        let input = br#"{"a":1,"b":"x"}"#;
        let tape = build_tape(input).unwrap();
        let kinds: Vec<u8> = tape.entries.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                KIND_OBJ_START,
                KIND_KEY,
                KIND_NUMBER,
                KIND_KEY,
                KIND_STRING,
                KIND_OBJ_END
            ]
        );
        // OBJ_START.link points at the matching OBJ_END (last entry).
        assert_eq!(tape.entries[0].link as usize, tape.entries.len() - 1);
        // OBJ_END.link points back at OBJ_START.
        assert_eq!(
            *tape.entries.last().unwrap(),
            TapeEntry {
                offset: tape.entries.last().unwrap().offset,
                kind: KIND_OBJ_END,
                link: 0
            }
        );
    }

    /// Nested structure — an array of objects. Each inner OBJ_START
    /// must have its link pointing at its OWN OBJ_END, not the outer
    /// ARR_END. This is the invariant Phase 3 (lazy indexed access)
    /// relies on to skip past unwanted elements.
    #[test]
    fn tape_nested_array_of_objects() {
        let input = br#"[{"a":1},{"b":2},{"c":3}]"#;
        let tape = build_tape(input).unwrap();
        // ARR_START ... ARR_END outer
        assert_eq!(tape.entries[0].kind, KIND_ARR_START);
        assert_eq!(tape.entries.last().unwrap().kind, KIND_ARR_END);
        // Three object children — count OBJ_START entries.
        let n_objs = tape
            .entries
            .iter()
            .filter(|e| e.kind == KIND_OBJ_START)
            .count();
        assert_eq!(n_objs, 3);
        // Each OBJ_START's link points at an OBJ_END strictly before ARR_END.
        for (i, e) in tape.entries.iter().enumerate() {
            if e.kind == KIND_OBJ_START {
                let end = e.link as usize;
                assert!(end > i, "OBJ_START.link must point forward");
                assert!(
                    end < tape.entries.len() - 1,
                    "OBJ_END must precede outer ARR_END"
                );
                assert_eq!(tape.entries[end].kind, KIND_OBJ_END);
                assert_eq!(
                    tape.entries[end].link as usize, i,
                    "OBJ_END.link must point back"
                );
            }
        }
    }

    /// Escaped string in a key and value — tape should still emit
    /// one KEY and one STRING entry; string decoding is deferred to
    /// materialization and doesn't perturb the tape shape.
    #[test]
    fn tape_escaped_strings() {
        let input = br#"{"a\"b":"x\\y"}"#;
        let tape = build_tape(input).unwrap();
        assert_eq!(
            tape.entries.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![KIND_OBJ_START, KIND_KEY, KIND_STRING, KIND_OBJ_END]
        );
    }

    /// Malformed inputs must return None (caller falls back to
    /// direct parser with richer error messages).
    #[test]
    fn tape_malformed_returns_none() {
        assert!(build_tape(b"{").is_none(), "unclosed object");
        assert!(build_tape(b"[").is_none(), "unclosed array");
        assert!(build_tape(b"{a:1}").is_none(), "unquoted key");
        assert!(build_tape(b"{\"a\"}").is_none(), "missing colon");
        assert!(build_tape(b"").is_none(), "empty input");
    }

    /// Top-level scalar (allowed by JSON spec).
    #[test]
    fn tape_top_level_scalars() {
        assert_eq!(build_tape(b"42").unwrap().entries.len(), 1);
        assert_eq!(build_tape(b"true").unwrap().entries.len(), 1);
        assert_eq!(build_tape(br#""hi""#).unwrap().entries.len(), 1);
        assert_eq!(build_tape(b"null").unwrap().entries.len(), 1);
    }

    /// `TapeEntry` is 12 bytes (u32 + u8 + padding + u32). Keeping
    /// this compact matters for tape-size parity with parse output:
    /// a 1 MB JSON blob with ~20k tokens should build a ~240 KB tape,
    /// not a megabyte.
    #[test]
    fn tape_entry_layout() {
        assert!(
            std::mem::size_of::<TapeEntry>() <= 12,
            "TapeEntry grew beyond 12 bytes — check padding"
        );
    }

    #[test]
    fn force_materialize_numeric_lazy_array_preserves_raw_payload() {
        let input = br#"[1,2.5,3]"#;
        let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let lazy = with_built_tape(input, |tape| unsafe {
            alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
        })
        .expect("valid JSON should build a tape");

        let arr = unsafe { force_materialize_lazy(lazy) };

        assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 1);
        assert_eq!(crate::array::js_array_numeric_get_f64_unboxed(arr, 0), 1.0);
        assert_eq!(crate::array::js_array_numeric_get_f64_unboxed(arr, 1), 2.5);
        assert_eq!(crate::array::js_array_numeric_get_f64_unboxed(arr, 2), 3.0);
        assert_eq!(
            crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
            Some(0)
        );
    }

    #[test]
    fn force_materialize_lazy_array_cache_downgrades_for_pointer_values() {
        let input = br#"[1,2,3]"#;
        let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let lazy = with_built_tape(input, |tape| unsafe {
            alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
        })
        .expect("valid JSON should build a tape");

        unsafe {
            let cached = crate::string::js_string_from_bytes(b"cached".as_ptr(), 6);
            *(*lazy).materialized_elements.add(1) =
                JSValue::string_ptr(cached as *mut crate::StringHeader);
            *(*lazy).materialized_bitmap |= 1u64 << 1;
        }

        let arr = unsafe { force_materialize_lazy(lazy) };

        assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 0);
        assert_eq!(
            crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
            Some(1)
        );
    }
}

impl PartialEq for TapeEntry {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset && self.kind == other.kind && self.link == other.link
    }
}

// ─── Phase 2 + 4: Lazy array header ───────────────────────────────────────────
//
// Representation for a `JSON.parse(blob)` top-level array that
// hasn't been materialized yet. Arena-allocated (same fast-alloc
// path as regular arrays), distinguished by `GcHeader::obj_type ==
// GC_TYPE_LAZY_ARRAY`. The accessor contract:
//
// - `js_array_length` on a lazy pointer returns `cached_length`
//   without touching the tape — O(1), no materialization.
// - Every other array accessor calls `force_materialize_lazy` to
//   lower the lazy value to a real `ArrayHeader`-backed tree, then
//   delegates to the generic path. Once materialized, the tape path
//   is dead for this value.
// - `js_json_stringify` checks `materialized.is_null()` — if true,
//   memcpys the original blob bytes (Phase 4 fast path); if false,
//   walks the materialized tree.
//
// The inline tape bytes (after the header, within the same arena
// allocation) get reclaimed with the header on the next arena block
// reset — same lifetime as any arena object.

/// Magic sentinel — paired with `obj_type == GC_TYPE_LAZY_ARRAY` as
/// a defensive double-check during accessor dispatch.
pub const LAZY_ARRAY_MAGIC: u32 = 0x4C5A5841; // "LZXA"

#[repr(C)]
pub struct LazyArrayHeader {
    /// **Offset 0 is load-bearing**: Perry's codegen inlines `.length`
    /// reads as a raw `u32` load at offset 0 (it doesn't go through
    /// `js_array_length`). Putting `cached_length` here means the
    /// inline-length fast path on an unmaterialized lazy array
    /// returns the right number without any runtime-function call.
    /// This layout choice is the whole reason the Phase 2 .length
    /// fast path is observable in the benchmark.
    pub cached_length: u32,
    /// Offset 4: magic sentinel. Also happens to sit where
    /// `ArrayHeader::capacity` lives on a regular array, so
    /// `clean_arr_ptr`'s `length > capacity` sanity check passes
    /// (cached_length is always < magic). Accessors that want to
    /// distinguish lazy from non-lazy arrays read
    /// `GcHeader::obj_type` (see `clean_arr_ptr` + `js_array_length`).
    pub magic: u32,
    /// Tape index where the root ARR_START sits.
    pub root_idx: u32,
    /// Number of `TapeEntry`s that follow inline after this header.
    pub tape_len: u32,
    /// Owns-a-reference to the input `StringHeader`. GC must trace
    /// this to keep the blob alive while this lazy value is
    /// reachable.
    pub blob_str: *const crate::StringHeader,
    /// Null until a *full-array* operation forces materialization
    /// (mutation, iteration, spread, .map, etc.). Once non-null, the
    /// value behaves exactly like a regular array and the sparse
    /// per-element cache below is effectively dead.
    pub materialized: *mut crate::array::ArrayHeader,
    /// Phase 5: sparse per-element cache. `materialized_elements[i]`
    /// is only meaningful when the corresponding bit in
    /// `materialized_bitmap` is set. `JSValue::ZERO` is a valid value
    /// (number 0 bits are all zero under NaN-boxing), so the bitmap
    /// is the authoritative "cache valid" signal — we can't use
    /// null-pointer semantics here.
    ///
    /// Identity invariant: a cache hit returns the *same* JSValue on
    /// every access, so `parsed[i] === parsed[i]` holds. Without
    /// this cache we'd return two distinct materialized objects and
    /// user code that stores `parsed[0]` into a variable then
    /// compares it against `parsed[0]` later would see `false`.
    pub materialized_elements: *mut crate::value::JSValue,
    /// 1 bit per index, `ceil(cached_length / 64)` words. Set when
    /// the corresponding slot in `materialized_elements` holds a
    /// valid materialized JSValue.
    pub materialized_bitmap: *mut u64,
    /// Walk cursor: the top-level element index we most recently
    /// visited, and the tape offset it lives at. Lets sequential
    /// access (`for i in 0..len { parsed[i] }`) walk in O(1) per
    /// step instead of O(n²) from the root. `walk_idx == u32::MAX`
    /// means "no prior walk" — start from root+1.
    ///
    /// Invariant: if `walk_idx != u32::MAX`, then `walk_tape_pos`
    /// points at the tape entry for the element at `walk_idx`.
    /// Updated at the end of every `lazy_get` call on a cold path.
    pub walk_idx: u32,
    pub walk_tape_pos: u32,
    /// Cumulative tape steps walked across all cold-path `lazy_get`
    /// calls on this header. When this exceeds `2 × cached_length`,
    /// we've spent enough on per-element walks that full-
    /// materializing (O(cached_length)) is cheaper for future
    /// accesses — trigger it and route subsequent reads through the
    /// `ArrayHeader` tree. This is the "random access" adaptive
    /// fallback: sequential walks stay at ~1 step per element and
    /// never trip; random walks average n/2 steps and trip after
    /// ~4 accesses on a 10k-element array, flipping to O(1) access
    /// and saving 50-100× on the rest of the workload.
    pub cumulative_walk_steps: u64,
    // Followed by `tape_len` `TapeEntry` elements inline.
}

impl LazyArrayHeader {
    /// Slice view over the inline tape bytes. Caller must keep the
    /// header alive for the slice's lifetime.
    #[inline]
    pub unsafe fn tape_slice<'a>(this: *const LazyArrayHeader) -> &'a [TapeEntry] {
        let base =
            (this as *const u8).add(std::mem::size_of::<LazyArrayHeader>()) as *const TapeEntry;
        std::slice::from_raw_parts(base, (*this).tape_len as usize)
    }

    /// Slice view over the blob bytes (data portion of the
    /// `StringHeader`). Caller must keep `blob_str` alive.
    #[inline]
    pub unsafe fn blob_bytes<'a>(this: *const LazyArrayHeader) -> &'a [u8] {
        let s = (*this).blob_str;
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, len)
    }
}

/// Arena-allocate a lazy array header with `tape_entries` copied
/// inline after the header. Returns the pointer that `JSON.parse`
/// hands back as a POINTER_TAG'd JSValue.
pub unsafe fn alloc_lazy_array(
    tape_entries: &[TapeEntry],
    root_idx: u32,
    cached_length: u32,
    blob_str: *const crate::StringHeader,
) -> *mut LazyArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let blob_handle = scope.root_string_ptr(blob_str);
    let tape_bytes = std::mem::size_of_val(tape_entries);
    let total = std::mem::size_of::<LazyArrayHeader>() + tape_bytes;
    let raw = crate::arena::arena_alloc_gc(total, 8, crate::gc::GC_TYPE_LAZY_ARRAY);
    let hdr = raw as *mut LazyArrayHeader;
    (*hdr).cached_length = cached_length;
    (*hdr).magic = LAZY_ARRAY_MAGIC;
    (*hdr).root_idx = root_idx;
    (*hdr).tape_len = tape_entries.len() as u32;
    (*hdr).blob_str = blob_handle.get_raw_const_ptr::<crate::StringHeader>();
    (*hdr).materialized = std::ptr::null_mut();
    (*hdr).materialized_elements = std::ptr::null_mut();
    (*hdr).materialized_bitmap = std::ptr::null_mut();
    (*hdr).walk_idx = u32::MAX;
    (*hdr).walk_tape_pos = 0;
    (*hdr).cumulative_walk_steps = 0;
    let hdr_handle = scope.root_raw_mut_ptr(hdr);
    json_tape_safepoint(JsonTapeSafepoint::LazyArrayRooted, hdr as usize);
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    (*hdr).blob_str = blob_handle.get_raw_const_ptr::<crate::StringHeader>();
    note_lazy_raw_slot(
        hdr,
        &(*hdr).blob_str as *const _ as usize,
        (*hdr).blob_str as usize,
    );
    // Allocate the sparse cache + bitmap in the arena so GC traces
    // them together with the header. The cache is an array of
    // `cached_length` JSValue slots; the bitmap is
    // `ceil(cached_length / 64)` u64 words. Both start zeroed
    // (arena_alloc_gc returns zeroed memory on fresh block), which
    // gives us empty bitmap + zeroed element slots — the invariant
    // being "cache slot is only valid when bitmap bit is set," so
    // the zero initial state is correctly "empty cache."
    //
    // For a 10k-record blob, cache = 80 KB + bitmap = 1.25 KB =
    // ~81 KB of per-parse overhead — small relative to the ~240 KB
    // tape itself.
    if cached_length > 0 {
        let cache_bytes = (cached_length as usize) * std::mem::size_of::<crate::value::JSValue>();
        let cache_raw = crate::arena::arena_alloc_gc(cache_bytes, 8, crate::gc::GC_TYPE_STRING);
        // arena_alloc_gc can reuse slots from the free list whose
        // bytes still hold whatever the previous occupant wrote.
        // Zero explicitly — the cache invariant relies on the
        // bitmap being the "cache valid" signal and the cache slots
        // starting clean; otherwise a leftover nonzero bit plus a
        // stale JSValue from a prior LazyArrayHeader gives us a
        // cross-parse ghost cache hit.
        std::ptr::write_bytes(cache_raw, 0, cache_bytes);
        let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
        (*hdr).materialized_elements = cache_raw as *mut crate::value::JSValue;
        note_lazy_raw_slot(
            hdr,
            &(*hdr).materialized_elements as *const _ as usize,
            cache_raw as usize,
        );
        let bitmap_words = (cached_length as usize).div_ceil(64);
        let bitmap_bytes = bitmap_words * 8;
        let bitmap_raw = crate::arena::arena_alloc_gc(bitmap_bytes, 8, crate::gc::GC_TYPE_STRING);
        std::ptr::write_bytes(bitmap_raw, 0, bitmap_bytes);
        let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
        (*hdr).materialized_bitmap = bitmap_raw as *mut u64;
        note_lazy_raw_slot(
            hdr,
            &(*hdr).materialized_bitmap as *const _ as usize,
            bitmap_raw as usize,
        );
    }
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    let tape_dst = (hdr as *mut u8).add(std::mem::size_of::<LazyArrayHeader>()) as *mut TapeEntry;
    // GC_STORE_AUDIT(POINTER_FREE): TapeEntry is offset/kind/link numerics, no heap edges.
    std::ptr::copy_nonoverlapping(tape_entries.as_ptr(), tape_dst, tape_entries.len());
    hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>()
}

#[inline]
unsafe fn note_lazy_raw_slot(hdr: *mut LazyArrayHeader, slot_addr: usize, child_addr: usize) {
    crate::gc::runtime_write_barrier_slot(hdr as usize, slot_addr, child_addr as u64);
}

/// Count top-level elements in the tape's root array. Hops forward
/// from `root_idx + 1` via the `link` field on container kinds to
/// skip nested subtrees — O(top-level-count), not O(total-nodes).
pub fn count_array_length(tape: &[TapeEntry], root_idx: usize) -> u32 {
    if root_idx >= tape.len() {
        return 0;
    }
    if tape[root_idx].kind != KIND_ARR_START {
        return 0;
    }
    let end = tape[root_idx].link as usize;
    let mut count: u32 = 0;
    let mut i = root_idx + 1;
    while i < end {
        let k = tape[i].kind;
        count += 1;
        if k == KIND_OBJ_START || k == KIND_ARR_START {
            i = tape[i].link as usize + 1;
        } else {
            i += 1;
        }
    }
    count
}

/// Phase 5: per-element sparse lookup. Return the i-th top-level
/// element of the lazy array, materializing only that element's
/// subtree on first access and caching the JSValue in the header's
/// sparse cache so `parsed[i] === parsed[i]` holds on subsequent
/// reads.
///
/// Fast path precedence:
/// 1. Full-materialize already happened (mutation, .map, etc.) →
///    forward to the regular ArrayHeader's inline element slot.
/// 2. Bitmap bit set → cache hit, return `materialized_elements[i]`.
/// 3. Cold read → walk the tape to the i-th entry via `link`
///    chasing, materialize that subtree, cache it, return.
///
/// Out-of-bounds returns `undefined`. Caller must ensure `hdr` is a
/// live LazyArrayHeader pointer; the materialize step uses the
/// arena allocator and may trigger GC (its `hdr` argument is
/// walked-through by the tracer if so, so the header survives).
pub unsafe fn lazy_get(hdr: *mut LazyArrayHeader, i: u32) -> JSValue {
    if hdr.is_null() {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let hdr_handle = scope.root_raw_mut_ptr(hdr);
    json_tape_safepoint(JsonTapeSafepoint::LazyGetHeaderRooted, hdr as usize);
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    if hdr.is_null() {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Fast path 1: full-materialize already triggered. Read from
    // the real array at arr+8+i*8.
    let mat = (*hdr).materialized;
    if !mat.is_null() {
        let length = (*mat).length;
        if i >= length {
            return JSValue::from_bits(crate::value::TAG_UNDEFINED);
        }
        let elements_ptr =
            (mat as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
        return JSValue::from_bits(*elements_ptr.add(i as usize));
    }

    let cached_length = (*hdr).cached_length;
    if i >= cached_length {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }

    // Fast path 2: bitmap hit.
    let bitmap = (*hdr).materialized_bitmap;
    let cache = (*hdr).materialized_elements;
    if !bitmap.is_null() && !cache.is_null() {
        let word_idx = (i as usize) / 64;
        let bit_idx = (i as usize) % 64;
        let word = *bitmap.add(word_idx);
        if word & (1u64 << bit_idx) != 0 {
            return *cache.add(i as usize);
        }
    }

    // Cold path: walk tape to entry i, materialize subtree, cache.
    let source = TapeSource::Lazy { hdr_handle };
    let root = (*hdr).root_idx as usize;
    let Some(root_entry) = source.entry(root) else {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    };
    if root_entry.kind != KIND_ARR_START {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    let end = root_entry.link as usize;

    // Walk cursor optimization: sequential access
    // (`for i in 0..len { parsed[i] }`) would otherwise be O(n²) —
    // 50M pointer chases for n=10k. If we previously visited index
    // `walk_idx` at tape offset `walk_tape_pos` and `i` is ahead of
    // it, resume walking from there. For the fully sequential
    // workload this amortizes to O(1) per step.
    let prev_walk = (*hdr).walk_idx;
    let start_count: u32;
    let mut idx: usize;
    if prev_walk != u32::MAX && i >= prev_walk {
        idx = (*hdr).walk_tape_pos as usize;
        start_count = prev_walk;
    } else {
        idx = root + 1;
        start_count = 0;
    }

    let mut element_count = start_count;
    while idx < end && element_count < i {
        let Some(entry) = source.entry(idx) else {
            return JSValue::from_bits(crate::value::TAG_UNDEFINED);
        };
        let k = entry.kind;
        if k == KIND_OBJ_START || k == KIND_ARR_START {
            idx = entry.link as usize + 1;
        } else {
            idx += 1;
        }
        element_count += 1;
    }
    if idx >= end {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }

    // Update cursor + cumulative walk counter. The step count for
    // this call is (i - start_count) at minimum (one step per
    // element) — container-skipping via `link` is O(1) per element
    // regardless of subtree size, so this bound matches the actual
    // work done.
    let step_cost = (i - start_count) as u64;
    (*hdr).walk_idx = i;
    (*hdr).walk_tape_pos = idx as u32;
    (*hdr).cumulative_walk_steps = (*hdr).cumulative_walk_steps.saturating_add(step_cost);

    let value = materialize_from_idx_source(&source, &scope, idx);
    let value_handle = scope.root_nanbox_u64(value.bits());
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    let bitmap = (*hdr).materialized_bitmap;
    let cache = (*hdr).materialized_elements;
    if !bitmap.is_null() && !cache.is_null() {
        let value_bits = value_handle.get_nanbox_u64();
        *cache.add(i as usize) = JSValue::from_bits(value_bits);
        crate::gc::runtime_write_barrier_slot(
            hdr as usize,
            cache.add(i as usize) as usize,
            value_bits,
        );
        let word_idx = (i as usize) / 64;
        let bit_idx = (i as usize) % 64;
        *bitmap.add(word_idx) |= 1u64 << bit_idx;
    }

    // Adaptive threshold: if cumulative walk steps exceed 2× the
    // array length, future per-element walks cost more than a
    // single full-materialize — trigger it now. Sequential access
    // (1 step per element) never trips; random access (n/2 per
    // step) trips after ~4 accesses on a 10k array. Post-trip,
    // every subsequent `lazy_get` hits the fast path at the top of
    // the function (materialized != null → direct ArrayHeader read).
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    if (*hdr).cumulative_walk_steps > (cached_length as u64) * 2 {
        force_materialize_lazy(hdr);
    }

    JSValue::from_bits(value_handle.get_nanbox_u64())
}

/// Force-materialize a lazy array into an `ArrayHeader`-backed tree.
/// Idempotent: subsequent calls return the cached `materialized`
/// pointer. Callers of array accessors that don't have a lazy path
/// invoke this first.
pub unsafe fn force_materialize_lazy(hdr: *mut LazyArrayHeader) -> *mut crate::array::ArrayHeader {
    if hdr.is_null() {
        return std::ptr::null_mut();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let hdr_handle = scope.root_raw_mut_ptr(hdr);
    json_tape_safepoint(JsonTapeSafepoint::ForceLazyHeaderRooted, hdr as usize);
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    if hdr.is_null() {
        return std::ptr::null_mut();
    }
    if !(*hdr).materialized.is_null() {
        return (*hdr).materialized;
    }
    let cached_length = (*hdr).cached_length;
    let bitmap = (*hdr).materialized_bitmap;
    let cache = (*hdr).materialized_elements;
    let has_cache_hits = if !bitmap.is_null() && !cache.is_null() && cached_length > 0 {
        let words = (cached_length as usize).div_ceil(64);
        let mut any = false;
        for w in 0..words {
            if *bitmap.add(w) != 0 {
                any = true;
                break;
            }
        }
        any
    } else {
        false
    };

    // Fast path: no cache hits — the tape is authoritative for
    // every element, walk it top-to-bottom.
    if !has_cache_hits {
        let source = TapeSource::Lazy { hdr_handle };
        let root = (*hdr).root_idx as usize;
        let js = materialize_from_idx_source(&source, &scope, root);
        let arr_handle = scope.root_nanbox_u64(js.bits());
        let arr_ptr = array_from_nanbox_handle(&arr_handle);
        let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
        (*hdr).materialized = arr_ptr;
        note_lazy_raw_slot(
            hdr,
            &(*hdr).materialized as *const _ as usize,
            arr_ptr as usize,
        );
        return arr_ptr;
    }

    // Slow path: the sparse cache may contain mutations. For each
    // top-level element, use the cached JSValue when bitmap bit is
    // set (preserves mutations + identity); otherwise materialize
    // from the tape. Build the array element-by-element.
    let arr_ptr = crate::array::js_array_alloc(cached_length);
    let arr_handle = scope.root_nanbox_u64(JSValue::object_ptr(arr_ptr as *mut u8).bits());
    json_tape_safepoint(JsonTapeSafepoint::ForceLazyArrayRooted, arr_ptr as usize);
    let source = TapeSource::Lazy { hdr_handle };
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    let root = (*hdr).root_idx as usize;
    if let Some(root_entry) = source.entry(root) {
        if root_entry.kind != KIND_ARR_START {
            let arr_ptr = array_from_nanbox_handle(&arr_handle);
            let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
            (*hdr).materialized = arr_ptr;
            note_lazy_raw_slot(
                hdr,
                &(*hdr).materialized as *const _ as usize,
                arr_ptr as usize,
            );
            return arr_ptr;
        }
        let end = root_entry.link as usize;
        let mut idx = root + 1;
        for i in 0..cached_length as usize {
            if idx >= end {
                break;
            }
            let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
            let bitmap = (*hdr).materialized_bitmap;
            let cache = (*hdr).materialized_elements;
            if bitmap.is_null() || cache.is_null() {
                break;
            }
            let word_idx = i / 64;
            let bit_idx = i % 64;
            let use_cache = (*bitmap.add(word_idx)) & (1u64 << bit_idx) != 0;
            let elem_scope = crate::gc::RuntimeHandleScope::new();
            let value = if use_cache {
                *cache.add(i)
            } else {
                let mut walk_idx = idx;
                materialize_value_source(&source, &elem_scope, &mut walk_idx)
            };
            let value_handle = elem_scope.root_nanbox_u64(value.bits());
            let arr_ptr = array_from_nanbox_handle(&arr_handle);
            let elements_ptr = (arr_ptr as *mut u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *mut u64;
            let value_bits = value_handle.get_nanbox_u64();
            // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
            *elements_ptr.add(i) = value_bits;
            (*arr_ptr).length = (i + 1) as u32;
            crate::array::note_array_slot(arr_ptr, i, value_bits);
            // Advance tape cursor past this element.
            let Some(entry) = source.entry(idx) else {
                break;
            };
            let k = entry.kind;
            if k == KIND_OBJ_START || k == KIND_ARR_START {
                idx = entry.link as usize + 1;
            } else {
                idx += 1;
            }
        }
    }
    let arr_ptr = array_from_nanbox_handle(&arr_handle);
    (*arr_ptr).length = cached_length;
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    (*hdr).materialized = arr_ptr;
    note_lazy_raw_slot(
        hdr,
        &(*hdr).materialized as *const _ as usize,
        arr_ptr as usize,
    );
    arr_ptr
}

/// Materialize starting from an arbitrary tape index — used by
/// `force_materialize_lazy`. Takes a borrowed slice and walks it in
/// place (no copy — the earlier implementation allocated a fresh
/// `Vec<TapeEntry>` on every force-materialize, which on a 10k-record
/// blob was ~600 KB of throwaway heap per indexed-read iteration
/// and showed up as a 2-3× slowdown on `bench_json_readonly_indexed`
/// vs the direct parser).
pub unsafe fn materialize_from_idx(tape: &[TapeEntry], bytes: &[u8], start_idx: usize) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = TapeSource::Borrowed { tape, bytes };
    materialize_from_idx_source(&source, &scope, start_idx)
}

unsafe fn materialize_from_idx_source(
    source: &TapeSource<'_, '_>,
    scope: &crate::gc::RuntimeHandleScope,
    start_idx: usize,
) -> JSValue {
    let mut idx = start_idx;
    materialize_value_source(source, scope, &mut idx)
}
