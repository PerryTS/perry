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

mod iterative;
pub(crate) use iterative::materialize_iterative;

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

    // Helper: validate and skip a JSON string in place (past the closing
    // quote). Decoding remains deferred to materialization.
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
                match bytes[*pos] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => *pos += 1,
                    b'u' => {
                        *pos += 1;
                        if *pos + 4 > bytes.len()
                            || !bytes[*pos..*pos + 4].iter().all(u8::is_ascii_hexdigit)
                        {
                            return false;
                        }
                        *pos += 4;
                    }
                    _ => return false,
                }
            } else if c < 0x20 {
                return false;
            } else {
                *pos += 1;
            }
        }
        false
    }

    // Helper: validate and skip a JSON number (past its last digit/exponent).
    #[inline(always)]
    fn skip_number(bytes: &[u8], pos: &mut usize) -> bool {
        if *pos < bytes.len() && bytes[*pos] == b'-' {
            *pos += 1;
        }
        match bytes.get(*pos) {
            Some(b'0') => *pos += 1,
            Some(b'1'..=b'9') => {
                *pos += 1;
                while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                    *pos += 1;
                }
            }
            _ => return false,
        }
        if *pos < bytes.len() && bytes[*pos] == b'.' {
            *pos += 1;
            let fraction_start = *pos;
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
            if *pos == fraction_start {
                return false;
            }
        }
        if *pos < bytes.len() && (bytes[*pos] == b'e' || bytes[*pos] == b'E') {
            *pos += 1;
            if *pos < bytes.len() && (bytes[*pos] == b'+' || bytes[*pos] == b'-') {
                *pos += 1;
            }
            let exponent_start = *pos;
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
            if *pos == exponent_start {
                return false;
            }
        }
        true
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
                        if !skip_number(bytes, &mut pos) {
                            return false;
                        }
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

    skip_ws(bytes, &mut pos);
    if pos != bytes.len() || !stack.is_empty() {
        return false;
    }
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
                let base = (*hdr).tape as *const TapeEntry;
                if base.is_null() {
                    return None;
                }
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
    let field_count = count_object_fields(source, *idx, end_idx);
    let obj = crate::object::js_object_alloc(0, 0);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    json_tape_safepoint(JsonTapeSafepoint::MaterializeObjectRooted, obj as usize);
    let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
    crate::object::reserve_object_spill(obj as usize, field_count);
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

/// Count only this object's keys, hopping over nested values through their
/// matching-container links. The walk allocates nothing, so it is safe for a
/// lazy source whose backing pointers may be refreshed through a GC handle.
unsafe fn count_object_fields(source: &TapeSource<'_, '_>, mut idx: usize, end_idx: usize) -> u32 {
    let mut count = 0u32;
    while idx < end_idx {
        let Some(key) = source.entry(idx) else {
            break;
        };
        if key.kind != KIND_KEY {
            break;
        }
        count = count.saturating_add(1);
        idx += 1;
        let Some(value) = source.entry(idx) else {
            break;
        };
        if value.kind == KIND_OBJ_START || value.kind == KIND_ARR_START {
            idx = value.link as usize + 1;
        } else {
            idx += 1;
        }
    }
    count
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
#[path = "json_tape_tests.rs"]
mod tests;

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
    /// Number of `TapeEntry`s reachable through [`Self::tape`].
    pub tape_len: u32,
    /// #7539: the tape's bytes, owned by `json_tape_store` rather than
    /// allocated inline after this header.
    ///
    /// **Not a GC edge.** This points at a plain `std::alloc` buffer, never at
    /// a managed object, so it is deliberately absent from the `LazyArray`
    /// rewrite descriptor: nothing marks it, nothing rewrites it, and no write
    /// barrier guards stores to it. The tape used to live inline, which made
    /// the whole allocation ~2.4 MB on a 10 k-record blob — over
    /// `LARGE_OBJECT_THRESHOLD_BYTES`, so `arena_alloc_gc` put it in the old
    /// generation with `GC_FLAG_TENURED`, where only a FULL collection can
    /// reclaim it. See `json_tape_store` for the measurement.
    ///
    /// Null once the tape is gone: either the blob had no entries, or
    /// `force_materialize_lazy` disowned it after installing `materialized`.
    /// Every reader checks `materialized.is_null()` before consulting the
    /// tape, so a null here is only ever observed as "length 0".
    pub tape: *mut TapeEntry,
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
    /// #7478: length of the current run of *consecutive ascending* cold
    /// reads (`parsed[k]`, `parsed[k+1]`, …). Reset to zero by any read
    /// that is not one past the previous one.
    ///
    /// `cumulative_walk_steps` provably cannot see a scan: a sequential
    /// walk costs exactly one tape step per element, so it accumulates
    /// `n` against a threshold of `2n` and never trips. A scan is
    /// therefore invisible to the only adaptive signal the header had,
    /// which is why `field_access` pays 10 000 element-wise
    /// materializations at ~1.8× the batch parser's cost for the same
    /// tree (#7478's quiet-host decomposition: 2540 ms of element-wise
    /// materialization against 1412 ms for a whole `DirectParser` parse,
    /// tokenization included). This counter is the missing signal;
    /// `scan_flip_threshold` is where it trips.
    pub sequential_streak: u32,
}

// `cached_length` at offset 0 is a CODEGEN contract, not a layout preference:
// Perry inlines `.length` as a raw u32 load at offset 0 rather than calling
// `js_array_length`, so an unmaterialized lazy array only reports the right
// length because this field sits first. Nothing else in the tree enforced
// that — the guarantee lived in a doc comment — so a field reordered into
// the front would have produced silently wrong `.length` values with every
// test still green. Adding a field to this struct is the moment that can
// happen, so pin it here.
const _: () = assert!(
    std::mem::offset_of!(LazyArrayHeader, cached_length) == 0,
    "LazyArrayHeader::cached_length must stay at offset 0 — codegen inlines \
     `.length` as a raw u32 load there"
);

/// #7478: how long a run of consecutive ascending cold reads has to get
/// before we stop materializing element-by-element and hand the whole
/// array to the batch parser.
///
/// The reparse rebuilds the WHOLE array, so with the element-wise
/// producer costing `r`× the batch one, flipping after a fraction `f` of
/// the array has been walked pays exactly when `f < 1 - 1/r`. At the
/// measured `r ≈ 1.8` that is `f < 44%`, which is why the caller pairs
/// this with `force_materialize_lazy`'s own `cached_count * 2 <
/// cached_length` (`f < 50%`) gate rather than relying on the streak
/// alone.
///
/// A streak is the evidence that `f` will keep growing, and the threshold
/// scales with the array so that evidence stays proportional: 1/64th of
/// the elements, floored at 64 so small arrays are not flipped on a
/// glance. The floor protects the "peek at a handful of records" shape —
/// `parsed[0]`..`parsed[9]` on a 10k array must NOT drag in a full parse.
/// The waste is bounded on the other side too: if the scan stops right
/// after the flip we have done one batch parse, which is still cheaper
/// than the element-wise walk of the same array it replaced.
#[inline]
pub(crate) fn scan_flip_threshold(cached_length: u32) -> u32 {
    core::cmp::max(64, cached_length / 64)
}

/// How many elements are currently in the sparse cache.
///
/// `lazy_get`'s scan-flip trigger and `force_materialize_lazy`'s choice of
/// producer have to agree on this number — the trigger exists to ask for the
/// batch reparse, and asking when the callee will decline just materializes
/// the array early through the slow path. They therefore read it from one
/// place. The trigger used to approximate it as `i + 1`, which is only the
/// true count for a scan that starts at zero and touches nothing else.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader`.
#[inline]
unsafe fn lazy_cached_count(hdr: *const LazyArrayHeader) -> u64 {
    let bitmap = (*hdr).materialized_bitmap;
    let cached_length = (*hdr).cached_length;
    if bitmap.is_null() || (*hdr).materialized_elements.is_null() || cached_length == 0 {
        return 0;
    }
    let mut count: u64 = 0;
    for w in 0..(cached_length as usize).div_ceil(64) {
        count += (*bitmap.add(w)).count_ones() as u64;
    }
    count
}

impl LazyArrayHeader {
    /// Slice view over the tape bytes. Caller must keep the header alive for
    /// the slice's lifetime.
    ///
    /// Empty once the tape has been disowned (`materialized` installed) — the
    /// null check is what makes that state safe rather than a wild read.
    #[inline]
    pub unsafe fn tape_slice<'a>(this: *const LazyArrayHeader) -> &'a [TapeEntry] {
        let base = (*this).tape;
        if base.is_null() {
            return &[];
        }
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

/// Arena-allocate a lazy array header owning `tape_entries` as a side
/// allocation. Returns the pointer that `JSON.parse` hands back as a
/// POINTER_TAG'd JSValue.
///
/// #7539: the tape used to be copied INLINE after the header, making this one
/// allocation as large as the tape (~2.4 MB on a 10 k-record blob). That is
/// over `LARGE_OBJECT_THRESHOLD_BYTES`, so `arena_alloc_gc` routed it into the
/// old generation with `GC_FLAG_TENURED` and only a FULL collection could ever
/// reclaim it. The header is ~88 bytes now and is born in the nursery like any
/// other short-lived object; `json_tape_store` owns the tape bytes.
/// The header's own bytes.
///
/// **Old generation, born tenured — and that is load-bearing, not incidental.**
/// Before #7539 the header carried its tape inline, so it was multi-megabyte
/// and `arena_alloc_gc`'s large-object arm put it here; every caller outside
/// this module has therefore always been free to hold a raw
/// `*mut LazyArrayHeader` across an allocation, and several do
/// (`json::stringify_api::try_stringify_lazy_array` reads `blob_bytes` off a
/// raw header and then allocates the result string; the array accessors pass
/// raw headers into `force_materialize_lazy`).
///
/// Shrinking the header to ~88 bytes without pinning it here made it
/// nursery-resident and therefore MOVABLE for the first time, and the copying
/// minor promptly relocated it out from under those callers: `field_access`
/// went non-deterministic, emitting a JSON string of NUL bytes for
/// `JSON.stringify(parsed)` on 3 of 60 iterations (a stale `blob_str` read
/// through a moved-from header). Keeping the header exactly where it has
/// always been costs ~96 bytes of old generation per parse — the tape's
/// ~2.4 MB is what had to leave — and keeps that contract intact.
#[inline]
fn alloc_lazy_header_bytes() -> *mut u8 {
    crate::arena::arena_alloc_gc_old_born_tenured(
        std::mem::size_of::<LazyArrayHeader>(),
        8,
        crate::gc::GC_TYPE_LAZY_ARRAY,
    )
}

pub unsafe fn alloc_lazy_array(
    tape_entries: &[TapeEntry],
    root_idx: u32,
    cached_length: u32,
    blob_str: *const crate::StringHeader,
) -> *mut LazyArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let blob_handle = scope.root_string_ptr(blob_str);
    // Detach the tape FIRST, while there is no header address to invalidate.
    // The buffer itself is plain `std::alloc` memory — no arena accounting, no
    // collection. `allocate` does account the bytes as external side pressure,
    // which can trigger, but the only live thing we hold across it is
    // `blob_handle`, which is rooted.
    let (tape_ptr, tape_allocation) = crate::json_tape_store::allocate(tape_entries);
    let (raw, blob_str) =
        blob_handle.across_const::<crate::StringHeader, _>(alloc_lazy_header_bytes);
    let hdr = raw as *mut LazyArrayHeader;
    (*hdr).cached_length = cached_length;
    (*hdr).magic = LAZY_ARRAY_MAGIC;
    (*hdr).root_idx = root_idx;
    (*hdr).tape_len = tape_entries.len() as u32;
    // GC_STORE_AUDIT(POINTER_FREE): side-allocated tape bytes, not a heap edge —
    // no barrier, and deliberately absent from the LazyArray rewrite descriptor.
    (*hdr).tape = tape_ptr;
    (*hdr).blob_str = blob_str;
    (*hdr).materialized = std::ptr::null_mut();
    (*hdr).materialized_elements = std::ptr::null_mut();
    (*hdr).materialized_bitmap = std::ptr::null_mut();
    (*hdr).walk_idx = u32::MAX;
    (*hdr).walk_tape_pos = 0;
    (*hdr).cumulative_walk_steps = 0;
    (*hdr).sequential_streak = 0;
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
        // Old-gen, like the header (#7539). Keeping the whole lazy-array
        // cluster in ONE generation keeps every edge out of it the shape
        // #7538/#7546 built and validated the external-slot barrier for:
        // old owner → old cache block → young element, recorded by
        // `note_lazy_cache_slot` and consumed by the minor's dirty scan
        // through the owner's descriptor. A nursery cache under an old-gen
        // header is a mixed shape nothing covers — the minor treats the old
        // header as a black leaf, so it never visits the descriptor that can
        // read the cache, while the cache block itself is a GC leaf whose
        // contents no walker scans. That combination lost element identity
        // (`parsed[i] === parsed[i]`) across a copying minor. It could not
        // occur before: a big array's cache was already born old, and a small
        // array's header was born young along with its cache.
        let cache_raw = crate::arena::arena_alloc_gc_old_born_tenured(
            cache_bytes,
            8,
            crate::gc::GC_TYPE_STRING,
        );
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
        // Same generation as the header and cache — see above. The bitmap
        // holds no heap edges, but keeping it with its cluster keeps the
        // page-liveness bookkeeping uniform.
        let bitmap_raw = crate::arena::arena_alloc_gc_old_born_tenured(
            bitmap_bytes,
            8,
            crate::gc::GC_TYPE_STRING,
        );
        std::ptr::write_bytes(bitmap_raw, 0, bitmap_bytes);
        let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
        (*hdr).materialized_bitmap = bitmap_raw as *mut u64;
        note_lazy_raw_slot(
            hdr,
            &(*hdr).materialized_bitmap as *const _ as usize,
            bitmap_raw as usize,
        );
    }
    // Register LAST, off the rooted handle. The header is old-gen and
    // immovable (see `alloc_lazy_header_bytes`), so this address is stable for
    // its whole life — which is what lets the registry be keyed by it with no
    // move hook.
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    crate::json_tape_store::register(hdr as usize, tape_allocation);
    hdr
}

/// Install `arr_ptr` as this header's materialized array and disown the tape.
///
/// Every site that sets `materialized` goes through here so the release can
/// never drift away from the install — the tape is garbage from the instant
/// `materialized` is non-null (`lazy_get`'s first fast path returns out of the
/// `ArrayHeader` and never consults the tape or the sparse cache again), and a
/// site that set the field directly would silently retain ~2.4 MB per parse
/// again.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader` and `arr_ptr` a live `ArrayHeader`.
/// No `TapeSource::Lazy` read of this header may be in flight.
#[inline]
unsafe fn install_materialized(hdr: *mut LazyArrayHeader, arr_ptr: *mut crate::array::ArrayHeader) {
    (*hdr).materialized = arr_ptr;
    note_lazy_raw_slot(
        hdr,
        &(*hdr).materialized as *const _ as usize,
        arr_ptr as usize,
    );
    release_tape_after_materialize(hdr);
}

/// Disown the tape once `materialized` is installed.
///
/// After a full materialization every read goes through the `ArrayHeader`, so
/// the tape is provably garbage at this exact instant — no collector has to
/// prove it. Freeing here is what keeps `field_access` flat: #7537 flips a
/// scan to the batch parser after `scan_flip_threshold` elements, so the
/// ~2.4 MB tape becomes dead within the first few hundred of 10 000 reads and
/// is released immediately rather than waiting for the next full collection.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader` whose `materialized` field is
/// already non-null, and no `TapeSource::Lazy` borrow of its tape may be live.
pub(crate) unsafe fn release_tape_after_materialize(hdr: *mut LazyArrayHeader) {
    if hdr.is_null() || (*hdr).materialized.is_null() || (*hdr).tape.is_null() {
        return;
    }
    crate::json_tape_store::release(hdr as usize);
    // GC_STORE_AUDIT(POINTER_FREE): clears the side-allocation pointer after deregistration.
    (*hdr).tape = std::ptr::null_mut();
    (*hdr).tape_len = 0;
}

#[inline]
unsafe fn note_lazy_raw_slot(hdr: *mut LazyArrayHeader, slot_addr: usize, child_addr: usize) {
    crate::gc::runtime_write_barrier_slot(hdr as usize, slot_addr, child_addr as u64);
}

/// Barrier for a store into the sparse element cache (#7538).
///
/// The cache is NOT part of the `LazyArrayHeader` allocation — it is a
/// separate `GC_TYPE_STRING` block hanging off `materialized_elements`, and at
/// ≥2049 elements (`cached_length * 8 > LARGE_OBJECT_THRESHOLD_BYTES`) it is
/// born directly in old-gen. That makes the ordinary in-object barrier
/// ([`note_lazy_raw_slot`]) the WRONG one here, and silently so:
/// `remember_old_to_young_slot` marks the page the SLOT lives on, and the
/// minor's dirty-page scan then walks the objects on that page and finds the
/// cache's own `GC_TYPE_STRING` header — a GC leaf with no child slots, so it
/// scans nothing. The only descriptor that can read those slots is
/// `GcRewriteDescriptorKind::LazyArray`, which hangs off the OWNER header,
/// whose pages stay clean. A copying minor therefore neither marked nor
/// rewrote the cached element pointers: element identity survived (the bitmap
/// still says "cached"), but the pointer named a retired from-space copy.
/// Reading a record through it returned the pre-collection `keys_array`, which
/// is why `JSON.stringify` emitted `{"field0":…,"field1":…}` for exactly one
/// record per run with the values still correct.
///
/// `runtime_write_barrier_external_slot` is the out-of-object form: it records
/// `(slot page → owner header)` so the scan re-enters through the owner's
/// LazyArray descriptor, which is what
/// `test_dirty_lazy_array_external_cache_scan_marks_bitmap_selected_child`
/// has always exercised — from a hand-planted entry no producer ever wrote.
#[inline]
unsafe fn note_lazy_cache_slot(hdr: *mut LazyArrayHeader, slot_addr: usize, value_bits: u64) {
    crate::gc::runtime_write_barrier_external_slot(hdr as usize, slot_addr, value_bits);
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
    let bitmap = (*hdr).materialized_bitmap;
    let cache = (*hdr).materialized_elements;

    // Fast path 2: bitmap hit.
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
    // #7478: extend the consecutive-ascending run, or start a new one. A
    // cold read of index 0 with no prior walk opens a streak; any read
    // that is not exactly one past the previous COLD read ends it. Cache
    // hits never reach here, so a re-scan of an already-materialized
    // prefix does not inflate the count.
    let streak = if prev_walk != u32::MAX && i == prev_walk + 1 {
        (*hdr).sequential_streak.saturating_add(1)
    } else {
        // A cold read that does not continue the previous run still IS a run
        // — of length one. Recording zero here made the run lag by one, so a
        // scan that began anywhere but index 0 needed 65 reads to trip a
        // threshold of 64, and a lone read reported a shorter run than the
        // very next read that continued it.
        1
    };
    (*hdr).sequential_streak = streak;
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
        note_lazy_cache_slot(hdr, cache.add(i as usize) as usize, value_bits);
        let word_idx = (i as usize) / 64;
        let bit_idx = (i as usize) % 64;
        *bitmap.add(word_idx) |= 1u64 << bit_idx;
    }

    // Adaptive thresholds. Either signal means future per-element walks
    // cost more than one full materialize, so trigger it now; afterwards
    // every `lazy_get` hits fast path 1 at the top of the function
    // (materialized != null → direct ArrayHeader read).
    //
    // 1. Cumulative walk steps past 2× the array length. Random access
    //    averages n/2 steps per read, so this trips after ~4 reads on a
    //    10k array. Sequential access costs 1 step per element and
    //    provably never trips it — which is the #7478 hole.
    // 2. A consecutive-ascending streak past `scan_flip_threshold`. This
    //    is the scan-shaped signal the first one cannot see. It fires
    //    while the sparse cache is still nearly empty, which is what
    //    makes `force_materialize_lazy` take #7499's batch reparse
    //    (gated on `cached_count * 2 < cached_length`) instead of the
    //    element-wise merge walk. Firing it late — after the scan has
    //    already filled the bitmap — is a no-op, which is exactly why
    //    #7499 alone did not move this benchmark.
    //
    // The second signal carries the SAME "is the batch producer even going
    // to be picked?" test that `force_materialize_lazy` applies, read from
    // the same `lazy_cached_count` helper. It used to approximate the cache
    // count as `i + 1`, which is only true for a scan that starts at zero
    // and touches nothing else: any earlier cold read outside the prefix
    // made the trigger UNDERcount, so it could fire on an array the callee
    // then declined to reparse — materializing the whole thing early
    // through the element-wise merge walk, the exact path this is meant to
    // avoid. The popcount is O(n/64) and only runs once the streak has
    // already reached the threshold, which for a scan happens once.
    //
    // It is also what stops the flip firing on an array whose streak can
    // only complete near the end — a 64- to 128-element one, where the
    // flip would land on the last read and reparse a tree the merge walk
    // was already holding.
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    let scan_flip = streak >= scan_flip_threshold(cached_length)
        && lazy_cached_count(hdr) * 2 < cached_length as u64;
    if (*hdr).cumulative_walk_steps > (cached_length as u64) * 2 || scan_flip {
        force_materialize_lazy(hdr);
    }

    JSValue::from_bits(value_handle.get_nanbox_u64())
}

thread_local! {
    /// #7478 witness: how many lazy arrays this thread batch-materialized
    /// by RE-PARSING the retained blob rather than walking the tape. A
    /// test that only asserts "the values came out right" cannot tell the
    /// two producers apart — this is what lets it assert its subject ran.
    static REPARSE_MATERIALIZATIONS: Cell<u64> = const { Cell::new(0) };
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn reparse_materializations() -> u64 {
    REPARSE_MATERIALIZATIONS.with(|c| c.get())
}

/// #7478: batch-materialize a lazy array by RE-PARSING its retained blob
/// with the `DirectParser`, instead of walking the tape element by
/// element.
///
/// A lazy array only ever stands for a TOP-LEVEL array — `try_parse_via_tape`
/// builds one only when `tape_entries[0].kind == KIND_ARR_START`, and always
/// with `root_idx = 0` — so `blob_str` is exactly this array's source text
/// and a fresh `DirectParser::parse_value()` over it reproduces the identical
/// tree. Identical includes the numbers: #7483 put the DirectParser's decimal
/// fast path on one correctly-rounded division, so it now agrees bit-for-bit
/// with `str::parse::<f64>` (what `materialize_number` uses) and with node.
/// That divergence (#7477) is what blocked this change the first time.
///
/// The batch parser builds the same tree ~1.8× cheaper than the per-element
/// materializer (#7478's quiet-host decomposition on the 10k-record fixture:
/// 1412 ms for 50 whole `DirectParser` parses, tokenization included, against
/// 2540 ms of element-wise materialization for the same trees), because it
/// makes one linear pass instead of re-entering the walk, the sparse cache
/// and a fresh handle scope per element — and because it pre-sizes each
/// object from a known field count behind an inline hot-shape cache instead
/// of growing it a field at a time.
///
/// GC contract — this is the part the first attempt got wrong, and it
/// SIGSEGV'd intermittently for it. `DirectParser` is only sound inside a
/// no-move window, in three separate ways:
///   * it holds `input: &'a [u8]` derived from the blob's `StringHeader`
///     payload for the whole parse and cannot re-derive it;
///   * it carries an UNROOTED one-entry shape cache (`hot_shape_keys`,
///     `hot_shape_array`) — raw heap pointers the collector cannot see, the
///     "runtime-side cache of a raw heap pointer" shape;
///   * `array_push_parse_fast` fills fresh arrays through
///     `note_array_slot_layout_only`, which deliberately skips the
///     generational barrier on the strength of that same suppression.
/// `js_json_parse` buys all three with `gc_suppress()`; so must this. The
/// window here is a nesting-safe `GcSuppressScope`, because
/// `force_materialize_lazy` is reachable from inside stringify and the flat
/// `gc_unsuppress()` would end an outer window early.
///
/// Returns the refreshed header alongside the result: this function
/// allocates, so the caller's `hdr` is stale on EVERY exit, including the
/// declining ones.
unsafe fn reparse_materialize(
    scope: &crate::gc::RuntimeHandleScope,
    hdr_handle: &crate::gc::RuntimeHandle<'_>,
    hdr: *mut LazyArrayHeader,
    cached_length: u32,
) -> (Option<*mut crate::array::ArrayHeader>, *mut LazyArrayHeader) {
    // The blob is this array's own source only when the tape root is the
    // blob's first value. Every production lazy array is built that way;
    // anything else declines rather than guessing.
    if (*hdr).root_idx != 0 {
        return (None, hdr);
    }
    let blob = (*hdr).blob_str;
    if blob.is_null() {
        return (None, hdr);
    }
    let blob_len = (*blob).byte_len as usize;
    if blob_len == 0 {
        return (None, hdr);
    }

    // Nothing between this read of `blob` and the suppression window can
    // collect, so the slice derived inside it names the live payload.
    let saved_roots = crate::json::parse_root_save_len();
    let (parsed_bits, hdr) = hdr_handle.across_mut::<LazyArrayHeader, _>(|| {
        let _suppress = crate::gc::GcSuppressScope::new();
        let data = (blob as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, blob_len);
        let mut parser = crate::json::DirectParser::new(bytes);
        let parsed = parser.parse_value();
        // Hand the tree to PARSE_ROOTS before the window closes — the
        // handle-scope root below is pushed after it has already closed.
        crate::json::parse_root_push(parsed);
        parsed.bits()
    });
    let arr_handle = scope.root_nanbox_u64(parsed_bits);
    crate::json::parse_root_restore(saved_roots);

    // `clean_arr_ptr_mut` re-checks `obj_type == GC_TYPE_ARRAY`, so a
    // non-array or failed parse declines here instead of publishing a
    // bogus `materialized`. The length check is the same guard for a blob
    // whose tape and text somehow disagree.
    let arr_ptr = crate::array::clean_arr_ptr_mut(array_from_nanbox_handle(&arr_handle));
    if arr_ptr.is_null() || (*arr_ptr).length != cached_length {
        return (None, hdr);
    }

    let arr_addr = arr_ptr as usize;
    let (_, hdr) = hdr_handle.across_mut::<LazyArrayHeader, _>(|| {
        json_tape_safepoint(JsonTapeSafepoint::ForceLazyArrayRooted, arr_addr)
    });
    let arr_ptr = array_from_nanbox_handle(&arr_handle);

    // Patch the sparse cache back over the fresh slots. A cached slot holds
    // the JSValue user code already has a reference to and may have MUTATED
    // through it, so the cache — not the reparsed subtree — is authoritative
    // there, and its identity has to survive (`parsed[i] === parsed[i]`).
    //
    // `store_array_slot` is the store that knows how to downgrade a
    // RawF64-layout array when a pointer lands in it; a raw
    // `*elements.add(i) = bits` would leave the array flagged pointer-free
    // with a pointer inside it, which the tracer would never scan.
    //
    // The loop only stores, and runs inside a nesting-safe suppression
    // window, so `hdr` / `bitmap` / `cache` / `arr_ptr` stay valid for its
    // duration without a per-element re-read.
    {
        let _suppress = crate::gc::GcSuppressScope::new();
        let bitmap = (*hdr).materialized_bitmap;
        let cache = (*hdr).materialized_elements;
        if !bitmap.is_null() && !cache.is_null() {
            for w in 0..(cached_length as usize).div_ceil(64) {
                let mut word = *bitmap.add(w);
                while word != 0 {
                    let i = w * 64 + word.trailing_zeros() as usize;
                    word &= word - 1;
                    if i >= cached_length as usize {
                        break;
                    }
                    crate::array::store_array_slot(arr_ptr, i, (*cache.add(i)).bits());
                }
            }
        }
        install_materialized(hdr, arr_ptr);
    }
    REPARSE_MATERIALIZATIONS.with(|c| c.set(c.get().wrapping_add(1)));
    (Some(arr_ptr), hdr)
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
    let mut hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    if hdr.is_null() {
        return std::ptr::null_mut();
    }
    if !(*hdr).materialized.is_null() {
        return (*hdr).materialized;
    }
    let cached_length = (*hdr).cached_length;
    // Same helper `lazy_get`'s scan-flip trigger consults, so the trigger
    // can never ask for a producer this function then declines.
    let cached_count = lazy_cached_count(hdr);
    let has_cache_hits = cached_count > 0;

    // #7478: when most of the array still has to be built, re-parse the
    // retained blob with the batch DirectParser instead of walking the
    // tape element-by-element — same tree, ~1.8× cheaper. When MOST
    // elements are already cached the walk is the cheap producer (it
    // copies cached JSValues and materializes only the remainder), and a
    // reparse would rebuild subtrees it is about to throw away. Since the
    // reparse rebuilds the whole array, it pays exactly while the cached
    // fraction is below `1 - 1/1.8 ≈ 44%`; `cached_count * 2 <
    // cached_length` is that crossover rounded to a shift.
    if cached_count * 2 < cached_length as u64 {
        let (reparsed, refreshed) = reparse_materialize(&scope, &hdr_handle, hdr, cached_length);
        if let Some(arr) = reparsed {
            return arr;
        }
        // Declining still allocated, so take the refreshed header into the
        // tape walk below.
        hdr = refreshed;
    }

    // Fast path: no cache hits — the tape is authoritative for
    // every element, walk it top-to-bottom.
    if !has_cache_hits {
        let source = TapeSource::Lazy { hdr_handle };
        let root = (*hdr).root_idx as usize;
        let js = materialize_from_idx_source(&source, &scope, root);
        let arr_handle = scope.root_nanbox_u64(js.bits());
        let arr_ptr = array_from_nanbox_handle(&arr_handle);
        let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
        install_materialized(hdr, arr_ptr);
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
            install_materialized(hdr, arr_ptr);
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
    install_materialized(hdr, arr_ptr);
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
