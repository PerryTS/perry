//! Flat tape representation for lazy JSON arrays and iterative materialization.
//!
//! Tape entries contain only token kinds, byte offsets and subtree links.
//! Scanning validates syntax without constructing a managed value for each
//! token. Lazy array results retain the source and their tape, materializing
//! accessed elements or reparsing the full array when traversal warrants it.
//! The automatic parse route uses this for eligible array roots between 1 KB
//! and 16 MB; PERRY_JSON_TAPE can force or disable the route.
//!
//! Modest native scratch buffers are reused. Larger completed tapes transfer
//! into the result's exact-size side allocation; they do not retain a second
//! scratch copy. Eager and deeply nested materialization still borrow scratch.
//! Input byte borrows end before callbacks that may collect and move the input.

use crate::value::JSValue;
use std::cell::Cell;

mod cached_read;
pub use cached_read::{js_lazy_array_index_probe, lazy_get};
mod iterative;
mod scan;
pub(crate) use iterative::materialize_iterative;
use scan::{skip_number, skip_string, skip_ws};
mod mutation;
pub(crate) use mutation::set_lazy_index;

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
    SmallRecordBatchRooted,
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
    if build_tape_into::<false>(bytes, &mut entries, &mut stack, &mut 0, &mut 0) {
        Some(Tape { entries })
    } else {
        None
    }
}

/// Re-run the tape's iterative syntax scanner only after a deep parse fails.
/// Its byte offset is the same location the normal direct parser reports for
/// shallow input, without recursing on a document beyond the native bound.
pub(crate) fn malformed_offset(bytes: &[u8]) -> usize {
    let mut entries = Vec::new();
    let mut stack = Vec::new();
    let mut max_depth = 0;
    let mut offset = bytes.len();
    let _ = build_tape_into::<true>(bytes, &mut entries, &mut stack, &mut max_depth, &mut offset);
    offset
}

/// Build a tape into caller-provided storage. This is the hot-path
/// variant used by `JSON.parse` so repeated parse-churn workloads do
/// not allocate and free a fresh tape vector on every iteration.
// Keep both specializations outside the common parse entry and its stack frame.
#[inline(never)]
fn build_tape_into<const CAPTURE_DEPTH: bool>(
    bytes: &[u8],
    entries: &mut Vec<TapeEntry>,
    stack: &mut Vec<u32>,
    max_depth: &mut usize,
    error_pos: &mut usize,
) -> bool {
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

    macro_rules! invalid {
        () => {{
            *error_pos = pos;
            return false;
        }};
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
                        if CAPTURE_DEPTH {
                            *max_depth = (*max_depth).max(stack.len());
                            if *max_depth > crate::json::MAX_ITERATIVE_NESTING_DEPTH {
                                invalid!();
                            }
                        }
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
                                invalid!();
                            }
                            let key_off = pos as u32;
                            if !skip_string(bytes, &mut pos) {
                                invalid!();
                            }
                            entries.push(TapeEntry {
                                offset: key_off,
                                kind: KIND_KEY,
                                link: 0,
                            });
                            skip_ws(bytes, &mut pos);
                            if pos >= bytes.len() || bytes[pos] != b':' {
                                invalid!();
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
                        if CAPTURE_DEPTH {
                            *max_depth = (*max_depth).max(stack.len());
                            if *max_depth > crate::json::MAX_ITERATIVE_NESTING_DEPTH {
                                invalid!();
                            }
                        }
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
                            invalid!();
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
                            invalid!();
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
                            invalid!();
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
                            invalid!();
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
                            invalid!();
                        }
                        entries.push(TapeEntry {
                            offset: tok_off,
                            kind: KIND_NUMBER,
                            link: 0,
                        });
                        state = State::AfterValue;
                    }
                    _ => invalid!(),
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
                                invalid!();
                            }
                            let key_off = pos as u32;
                            if !skip_string(bytes, &mut pos) {
                                invalid!();
                            }
                            entries.push(TapeEntry {
                                offset: key_off,
                                kind: KIND_KEY,
                                link: 0,
                            });
                            skip_ws(bytes, &mut pos);
                            if pos >= bytes.len() || bytes[pos] != b':' {
                                invalid!();
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
                    _ => invalid!(),
                }
            }
        }
    }

    skip_ws(bytes, &mut pos);
    if pos != bytes.len() || !stack.is_empty() {
        invalid!();
    }
    if entries.is_empty() {
        invalid!();
    }
    true
}

/// Build a tape using thread-local scratch storage, then borrow the
/// completed entries for a caller-provided operation. Scratch is kept
/// only while it remains modest; large blobs are allowed to return
/// their backing allocation to the system allocator instead of pinning
/// a high-water tape buffer for the rest of the thread.
#[cfg(test)]
pub(crate) fn with_built_tape<R>(bytes: &[u8], f: impl FnOnce(&[TapeEntry]) -> R) -> Option<R> {
    unsafe { with_built_tape_raw(bytes.as_ptr(), bytes.len(), f) }
}

/// Build a tape before a callback that may relocate or release the source.
/// The input borrow ends with `build_tape_into`, before invoking `f`.
///
/// # Safety
/// `data` must describe `len` live readable bytes until `f` starts. The callback
/// must obtain a fresh source pointer after any collection; the tape itself
/// contains offsets, never source pointers. Nothing reads `data` after `f`.
pub(crate) unsafe fn with_built_tape_raw<R>(
    data: *const u8,
    len: usize,
    f: impl FnOnce(&[TapeEntry]) -> R,
) -> Option<R> {
    with_built_tape_mut_raw(data, len, |entries| f(entries))
}

/// As `with_built_tape_raw`, but the callback can transfer the completed Vec.
/// Only native tape storage is mutable; no source borrow crosses the callback.
///
/// # Safety
/// The source-pointer contract is the same as `with_built_tape_raw`.
pub(crate) unsafe fn with_built_tape_mut_raw<R>(
    data: *const u8,
    len: usize,
    f: impl FnOnce(&mut Vec<TapeEntry>) -> R,
) -> Option<R> {
    with_built_tape_depth_impl::<false, R>(data, len, |entries, _| f(entries))
}

/// Build a syntax-validated tape and capture the deepest container stack.
/// Refuse inputs beyond the parse resource budget before materialization.
/// The caller must retain the legacy depth/error check when this declines.
///
/// # Safety
/// As with `with_built_tape_raw`, no input borrow crosses the callback; `f`
/// receives only native entries and depth metadata, and must derive its source
/// pointer again from the root after any collection.
pub(crate) unsafe fn with_built_tape_depth_raw<R>(
    data: *const u8,
    len: usize,
    f: impl FnOnce(&mut Vec<TapeEntry>, usize) -> R,
) -> Option<R> {
    with_built_tape_depth_impl::<true, R>(data, len, f)
}

unsafe fn with_built_tape_depth_impl<const CAPTURE_DEPTH: bool, R>(
    data: *const u8,
    len: usize,
    f: impl FnOnce(&mut Vec<TapeEntry>, usize) -> R,
) -> Option<R> {
    TAPE_SCRATCH.with(|cell| {
        let mut scratch = cell.take().unwrap_or_else(TapeScratch::new);
        let mut max_depth = 0;
        let mut error_pos = 0;
        let built = build_tape_into::<CAPTURE_DEPTH>(
            std::slice::from_raw_parts(data, len),
            &mut scratch.entries,
            &mut scratch.stack,
            &mut max_depth,
            &mut error_pos,
        );
        let result = if built {
            Some(f(&mut scratch.entries, max_depth))
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

#[path = "json_tape/record_materialize.rs"]
mod record_materialize;

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
                    LazyArrayHeader::tape_slice(hdr).len()
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
                if hdr.is_null() {
                    return None;
                }
                LazyArrayHeader::tape_slice(hdr).get(idx).copied()
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
    // #8098: a lazily materialized tape record is `JSON.parse` output too — the
    // >1 KB top-level-array payloads (HTTP bodies, ORM result sets) that the
    // eager `DirectParser` never sees all arrive through here. Born marked
    // plain-ordinary (charter step 3: `mark_object_plain_ordinary`).
    let obj = crate::object::object_alloc_plain(0);
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
                crate::json::cache_parse_key(owned, p);
                return p;
            }
            let p =
                crate::string::js_string_from_bytes_longlived(slice.as_ptr(), slice.len() as u32);
            crate::json::cache_parse_key(slice.to_vec(), p);
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
    crate::json::cache_parse_key(key_bytes, p);
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
            if slice.len() <= crate::value::SHORT_STRING_MAX_LEN
                && !crate::string::bytes_have_lone_surrogate(slice)
            {
                if let Some(sso) = JSValue::try_short_string(slice) {
                    return sso;
                }
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
            if vec.len() <= crate::value::SHORT_STRING_MAX_LEN
                && !crate::string::bytes_have_lone_surrogate(&vec)
            {
                if let Some(sso) = JSValue::try_short_string(&vec) {
                    return sso;
                }
            }
            let ptr = crate::string::js_string_from_builder_bytes(&vec);
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
// Keep the scalar scan out of its materialization callers even though the
// canonical escape decoder is now shared and the scanner itself is small.
#[inline(never)]
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
            return parse_string_bytes_slow(bytes);
        }
        pos += 1;
    }
    None
}

// Keep the cold escape decoder shared with eager/batched parsing. In
// particular, lone surrogates must survive every lazy access order.
#[inline(never)]
fn parse_string_bytes_slow(bytes: &[u8]) -> Option<ParsedStr<'_>> {
    let mut parser = crate::json::DirectParser::new(bytes);
    match parser.parse_string_bytes()? {
        crate::json::ParsedStr::Borrowed(bytes) => Some(ParsedStr::Borrowed(bytes)),
        crate::json::ParsedStr::Owned(bytes) => Some(ParsedStr::Owned(bytes)),
    }
}

#[cfg(test)]
#[path = "json_tape/string_decode_tests.rs"]
mod string_decode_tests;

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
// - Every other array accessor calls `force_materialize_lazy`, which
//   fills the remaining element slots and turns the object into the
//   ordinary array IN PLACE (same address); the generic path then runs on
//   it. Once materialized, the value IS an Array (obj_type GC_TYPE_ARRAY)
//   and no lazy code sees it again.
// - `js_json_stringify` on a still-lazy value memcpys the original blob
//   bytes (Phase 4 fast path) when nothing cached can differ from them.
//
// The tape lives outside the GC heap (`json_tape_store`) and is released
// at materialization.

/// Magic sentinel — paired with `obj_type == GC_TYPE_LAZY_ARRAY` as
/// a defensive double-check during accessor dispatch.
pub const LAZY_ARRAY_MAGIC: u32 = 0x4C5A5841; // "LZXA"

#[repr(C)]
pub struct LazyArrayHeader {
    // ── The ArrayHeader prefix ─────────────────────────────────────────────
    // A lazy array is born with an ordinary array's header and element
    // storage, and becomes that array IN PLACE when it materializes
    // (`flip_to_array`): the GC header's `obj_type` turns from
    // GC_TYPE_LAZY_ARRAY into GC_TYPE_ARRAY and nothing else moves. One object,
    // one address, so identity (`===`, Map keys, WeakMap) and every compiled
    // binding stay valid across materialization, and a binding that saw the
    // lazy value takes the dense fast path afterwards. `layout.rs` pins these
    // offsets to `ArrayHeader`'s.
    /// `ArrayHeader::length`. Emitted code reads `.length` as a raw u32 here.
    pub cached_length: u32,
    /// `ArrayHeader::capacity`: the element slots after the lazy fields. Equal
    /// to `cached_length`.
    pub capacity: u32,
    /// `ArrayHeader::named`: always null (own properties materialize first).
    pub named: *mut crate::object::ObjectHeader,
    #[cfg(target_pointer_width = "32")]
    pub _ek_pad: u32,
    /// `ArrayHeader::ek_prefix` / `ek_kind`: zero, the weakest claims.
    pub ek_prefix: u32,
    pub ek_kind: u32,
    // ── Lazy fields: the array's front reserve ─────────────────────────────
    // `array_elements_ptr` addresses element zero as end-of-allocation minus
    // `capacity`, so the words between the prefix and the slots are front
    // reserve (storage.rs). After the flip they are dead slack that no
    // collector scans (an Array is traced over its logical element range).
    /// Magic sentinel, paired with `obj_type == GC_TYPE_LAZY_ARRAY`.
    pub magic: u32,
    /// Tape index where the root ARR_START sits.
    pub root_idx: u32,
    /// Number of `TapeEntry`s reachable through [`Self::tape`].
    pub tape_len: u32,
    /// #7478: length of the current run of *consecutive ascending* cold
    /// reads (`parsed[k]`, `parsed[k+1]`, …). Reset by any read that is not
    /// one past the previous one. A sequential walk costs one tape step per
    /// element, so `cumulative_walk_steps` can never see a scan; this is the
    /// signal `scan_flip_threshold` trips on.
    pub sequential_streak: u32,
    /// #7539: the tape's bytes, owned by `json_tape_store` (a plain
    /// `std::alloc` buffer, **not a GC edge**: absent from the LazyArray
    /// rewrite descriptor, no barrier). Null once the tape is gone.
    pub tape: *mut TapeEntry,
    /// Owns-a-reference to the input `StringHeader`. GC traces this to keep
    /// the blob alive while the array is lazy.
    pub blob_str: *const crate::StringHeader,
    /// Walk cursor: the top-level element index most recently visited and
    /// its tape offset, so a sequential walk is O(1) per step.
    /// `walk_idx == u32::MAX` means "no prior walk".
    pub walk_idx: u32,
    pub walk_tape_pos: u32,
    /// Cumulative tape steps over all cold reads. Past `2 × cached_length`
    /// a full materialization is cheaper than more random walks.
    pub cumulative_walk_steps: u64,
    /// How many element slots hold a materialized value (not `TAG_HOLE`).
    pub cached_count: u32,
    pub _pad: u32,
    // ── Element slots (`cached_length` words) follow ───────────────────────
    // A slot holds `TAG_HOLE` until its element is materialized (a JSON value
    // is never a hole), then that element's JSValue for good: a cached read
    // returns the same value every time, so `parsed[i] === parsed[i]` holds,
    // and the flip keeps every cached value in place.
}

mod layout;

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
    (*hdr).cached_count as u64
}

/// Element slot `i` of a lazy array: the slot the materialized array keeps.
///
/// Addressed by the array's own rule (end of allocation minus `capacity`,
/// `array_elements_ptr`), so the lazy value and the array it turns into can
/// never disagree about where element `i` lives, whatever the allocation's
/// padding.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader` and `i < cached_length`. Like any
/// GC interior pointer, the result must not be retained across a safepoint.
#[inline]
pub(crate) unsafe fn lazy_slot(hdr: *mut LazyArrayHeader, i: usize) -> *mut u64 {
    crate::array::array_elements_ptr(hdr as *const crate::array::ArrayHeader).add(i)
}

/// Write `TAG_HOLE` over the words between the `ArrayHeader` prefix and
/// element 0: the lazy fields, which become the array's front reserve.
/// The array keeps `[length, capacity)` and its reserve hole-filled (a reset
/// of the front offset hands the reserve back as capacity, storage.rs), so
/// no lazy word may outlive the flip as a readable element.
///
/// # Safety
///
/// `hdr` must be a live lazy array being flipped; no lazy field is read after.
#[inline]
unsafe fn hole_fill_front_reserve(hdr: *mut LazyArrayHeader) {
    let first = (hdr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
    let end = lazy_slot(hdr, 0);
    let mut word = first;
    while word < end {
        // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is not a heap pointer.
        word.write(crate::value::TAG_HOLE);
        word = word.add(1);
    }
}

/// The materialized element `i`, or `None` while its slot is still a hole.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader`.
#[inline]
pub(crate) unsafe fn lazy_cached_element(hdr: *mut LazyArrayHeader, i: u32) -> Option<JSValue> {
    if i >= (*hdr).cached_length {
        return None;
    }
    let bits = *lazy_slot(hdr, i as usize);
    (bits != crate::value::TAG_HOLE).then(|| JSValue::from_bits(bits))
}

/// Is `hdr` still a lazy array? False once `flip_to_array` turned it into the
/// ordinary array (same address).
///
/// # Safety
///
/// `hdr` must be a live GC user pointer.
#[inline]
pub(crate) unsafe fn is_lazy_array(hdr: *const LazyArrayHeader) -> bool {
    let header = (hdr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
}

impl LazyArrayHeader {
    /// Slice view over the tape bytes. Caller must keep the header alive for
    /// the slice's lifetime.
    ///
    /// Empty once the tape has been released, and empty once `flip_to_array`
    /// turned the value into an ordinary array: the lazy fields are then
    /// hole-filled front reserve, so the type test (not the field) is what
    /// makes a stale `TapeSource::Lazy` read safe rather than a wild read.
    #[inline]
    pub unsafe fn tape_slice<'a>(this: *const LazyArrayHeader) -> &'a [TapeEntry] {
        if !is_lazy_array(this) {
            return &[];
        }
        let base = (*this).tape;
        if base.is_null() {
            return &[];
        }
        std::slice::from_raw_parts(base, (*this).tape_len as usize)
    }

    /// Slice view over the blob bytes (data portion of the
    /// `StringHeader`). Caller must keep `blob_str` alive. Empty once the
    /// value flipped to an ordinary array (the blob edge is gone).
    #[inline]
    pub unsafe fn blob_bytes<'a>(this: *const LazyArrayHeader) -> &'a [u8] {
        if !is_lazy_array(this) {
            return &[];
        }
        let s = (*this).blob_str;
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, len)
    }
}

/// Keep the small lazy header old-gen, born tenured and immovable (#7539).
/// Its tape is a separate native allocation, so tape size no longer contributes
/// dead arena capacity. The registry keys ownership by the header address.
///
/// Before the tape moved out of line, the large-object allocator made every
/// header old-gen. Callers therefore hold raw header pointers across allocation
/// (`try_stringify_lazy_array`, lazy access and materialization). Making the
/// smaller header movable broke those contracts: copied minors left stale
/// `blob_str` reads and stringify emitted NUL bytes. Preserve its generation;
/// ownership transfer changes only the pointer-free tape backing allocation.
#[inline]
/// Does this lazy array's cluster belong in the old generation?
///
/// #7539 put the header there because its tape was inline and multi-megabyte;
/// moving the tape out shrank it to ~88 bytes and the old-gen request was kept
/// only because `json_tape_store` keyed a tape by its owner's address and the
/// copying minor's flip ran no finalize hook, so a young header would have
/// orphaned or leaked its tape. `GcMoveHookKind::LazyArrayTape` and
/// `finalize_dead_copied_minor_from_space_lazy_tapes` remove both reasons.
///
/// Pinning was expensive: a minor never sweeps old-gen, so a DEAD cluster held
/// its whole element graph live through the remembered set until a full
/// collection, which on a parse-and-scan loop never arrives.
///
/// Decide by size instead, and decide ONCE for the cluster: #7546's invariant
/// is that header, cache and bitmap share a generation, because a nursery cache
/// under an old-gen header is a mixed shape no walker covers. A cache large
/// enough to be born old takes the header with it; anything smaller is
/// nursery-resident and a minor can reclaim the lot.
fn lazy_cluster_is_old(cached_length: u32) -> bool {
    let cache_bytes = (cached_length as usize) * std::mem::size_of::<crate::value::JSValue>();
    // The POINTER-BEARING line, not the flat one. `arena_alloc_gc` keeps the two
    // apart for precisely the reason that bites here: tenuring a pointer-bearing
    // object does not cost its own bytes, it costs "every object it can reach,
    // held live through the remembered set by a container nothing refers to any
    // more". The sparse cache is a block of JSValues, so it is that container,
    // and a lazy array is the case the distinction was drawn for. 128 KB is
    // V8's kMaxRegularHeapObjectSize and sits inside the copier's own ceilings,
    // so a cluster admitted by it is always movable.
    cache_bytes + crate::gc::GC_HEADER_SIZE
        >= crate::gc::LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES
}

unsafe fn alloc_lazy_cluster_bytes(size: usize, obj_type: u8, old: bool) -> *mut u8 {
    if old {
        crate::arena::arena_alloc_gc_old_born_tenured(size, 8, obj_type)
    } else {
        crate::arena::arena_alloc_gc(size, 8, obj_type)
    }
}

pub unsafe fn alloc_lazy_array(
    tape_entries: &[TapeEntry],
    root_idx: u32,
    cached_length: u32,
    blob_str: *const crate::StringHeader,
) -> *mut LazyArrayHeader {
    alloc_lazy_array_backing(
        crate::json_tape_store::TapeBacking::Borrowed(tape_entries),
        root_idx,
        cached_length,
        blob_str,
    )
}

pub(crate) unsafe fn alloc_lazy_array_from_scratch(
    entries: &mut Vec<TapeEntry>,
    root_idx: u32,
    cached_length: u32,
    blob_str: *const crate::StringHeader,
) -> *mut LazyArrayHeader {
    let backing = if entries.capacity() * std::mem::size_of::<TapeEntry>()
        > MAX_RETAINED_TAPE_SCRATCH_BYTES
    {
        crate::json_tape_store::TapeBacking::Owned(std::mem::take(entries))
    } else {
        crate::json_tape_store::TapeBacking::Borrowed(entries)
    };
    alloc_lazy_array_backing(backing, root_idx, cached_length, blob_str)
}

unsafe fn alloc_lazy_array_backing(
    backing: crate::json_tape_store::TapeBacking<'_>,
    root_idx: u32,
    cached_length: u32,
    blob_str: *const crate::StringHeader,
) -> *mut LazyArrayHeader {
    let tape_len = backing.len() as u32;
    let scope = crate::gc::RuntimeHandleScope::new();
    let blob_handle = scope.root_string_ptr(blob_str);
    // Detach the tape FIRST, while there is no header address to invalidate.
    // The buffer itself is plain `std::alloc` memory — no arena accounting, no
    // collection. `allocate` does account the bytes as external side pressure,
    // which can trigger, but the only live thing we hold across it is
    // `blob_handle`, which is rooted.
    let (tape_ptr, tape_allocation) = backing.allocate();
    let cluster_old = lazy_cluster_is_old(cached_length);
    // One allocation: the ArrayHeader-compatible prefix, the lazy fields (the
    // array's front reserve) and `cached_length` element slots, which is
    // exactly the shape `flip_to_array` turns into an ordinary array.
    let size = std::mem::size_of::<LazyArrayHeader>()
        + (cached_length as usize) * std::mem::size_of::<crate::value::JSValue>();
    let (raw, blob_str) = blob_handle.across_const::<crate::StringHeader, _>(|| unsafe {
        alloc_lazy_cluster_bytes(size, crate::gc::GC_TYPE_LAZY_ARRAY, cluster_old)
    });
    let hdr = raw as *mut LazyArrayHeader;
    (*hdr).cached_length = cached_length;
    (*hdr).capacity = cached_length;
    // GC_STORE_AUDIT(POINTER_FREE): null edge in the fresh, unpublished header.
    (*hdr).named = std::ptr::null_mut();
    (*hdr).ek_prefix = 0;
    (*hdr).ek_kind = 0;
    (*hdr).magic = LAZY_ARRAY_MAGIC;
    (*hdr).root_idx = root_idx;
    (*hdr).tape_len = tape_len;
    // GC_STORE_AUDIT(POINTER_FREE): side-allocated tape bytes, not a heap edge —
    // no barrier, and deliberately absent from the LazyArray rewrite descriptor.
    (*hdr).tape = tape_ptr;
    (*hdr).blob_str = blob_str;
    (*hdr).walk_idx = u32::MAX;
    (*hdr).walk_tape_pos = 0;
    (*hdr).cumulative_walk_steps = 0;
    (*hdr).sequential_streak = 0;
    (*hdr).cached_count = 0;
    (*hdr)._pad = 0;
    // Every slot starts as a hole ("not materialized yet"). The allocator can
    // hand back reused bytes, so this write is also what keeps a previous
    // occupant's words from reading as cached elements.
    debug_assert!(
        lazy_slot(hdr, 0) as usize >= hdr as usize + std::mem::size_of::<LazyArrayHeader>()
    );
    // One slice fill off the slot base (a vectorizable store run, the
    // counterpart of the zeroing memset this replaced), not a per-index
    // `lazy_slot` address computation.
    // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is not a heap pointer.
    std::slice::from_raw_parts_mut(lazy_slot(hdr, 0), cached_length as usize)
        .fill(crate::value::TAG_HOLE);
    let hdr_handle = scope.root_raw_mut_ptr(hdr);
    json_tape_safepoint(JsonTapeSafepoint::LazyArrayRooted, hdr as usize);
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    (*hdr).blob_str = blob_handle.get_raw_const_ptr::<crate::StringHeader>();
    note_lazy_raw_slot(
        hdr,
        &(*hdr).blob_str as *const _ as usize,
        (*hdr).blob_str as usize,
    );
    // Register LAST, off the rooted handle. The header is old-gen and
    // immovable (see `alloc_lazy_header_bytes`), so this address is stable for
    // its whole life — which is what lets the registry be keyed by it with no
    // move hook.
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    crate::json_tape_store::register(hdr as usize, tape_allocation);
    hdr
}

/// Turn a fully materialized lazy array into the ordinary array it already
/// is, IN PLACE: same address, same element slots.
///
/// The prefix is an `ArrayHeader` (`layout.rs` pins the offsets) and the lazy
/// fields sit in its front reserve, so changing `obj_type` is the whole
/// conversion. Every holder of the value — compiled bindings, Map keys, a
/// `const keep = parsed` alias — sees the array from now on, and the dense
/// fast paths accept it because its header is an Array header. There is no
/// second object, so nothing has to be forwarded, repaired or compared
/// specially.
///
/// The tape is released here: after the flip no lazy reader can reach it.
/// The blob edge is cleared too; it lives in the front reserve, which the
/// collector does not scan for an Array, so the blob becomes garbage.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader` whose `cached_length` slots all hold
/// materialized values, with no `TapeSource::Lazy` borrow of its tape live.
unsafe fn flip_to_array(
    hdr: *mut LazyArrayHeader,
    all_pointers_proven: bool,
) -> *mut crate::array::ArrayHeader {
    release_tape(hdr);
    // The lazy fields (blob edge included) become hole-filled front reserve:
    // the blob is garbage from here, and no lazy word reads as an element.
    hole_fill_front_reserve(hdr);
    let header = (hdr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    (*header).obj_type = crate::gc::GC_TYPE_ARRAY;
    // A fresh Array's layout word starts at zero (the allocator's birth
    // state); `rebuild_array_layout` then classifies the slots it now holds
    // and records any old-to-young edge, exactly as for a bulk-filled array.
    // The object's own header facts survive: its copy age (the GC owns it)
    // and what the program already did to the lazy value (integrity level,
    // `Object.setPrototypeOf`'s `GC_ARRAY_CUSTOM_PROTO`).
    (*header)._reserved &= crate::gc::ARRAY_FLIP_KEPT_RESERVED;
    let arr = hdr as *mut crate::array::ArrayHeader;
    // Every slot now holds a materialized JSON value, and a JSON value is
    // never a hole: the whole range is the dense prefix (the EK birth row,
    // element_kind.rs). `ek_set_prefix` keeps the descriptor clamp for a
    // value the program already gave element descriptors.
    crate::array::ek_set_prefix(arr, 0, (*arr).length);
    if all_pointers_proven {
        // The caller copied every slot verbatim from a parser-built array
        // whose builder proved them all pointers (`ConstructionArray`), so
        // the classification `rebuild_array_layout` would scan for is known:
        // install it, and replay the range barrier for an old owner as the
        // rebuild does. (No element-shape proof to drop: a lazy value never
        // earns one, and the `_reserved` mask above keeps none.)
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
        if crate::arena::pointer_in_old_gen(arr as usize) {
            crate::gc::replay_old_parent_slot_range_barriers(
                arr as usize,
                crate::array::array_elements_ptr(arr),
                (*arr).length as usize,
            );
        }
    } else {
        crate::array::rebuild_array_layout(arr);
    }
    arr
}

/// Release the tape of a lazy array that is being materialized.
///
/// Freeing at the flip is what keeps `field_access` flat: #7537 flips a scan
/// to the batch parser after `scan_flip_threshold` elements, so the ~2.4 MB
/// tape is released within the first few hundred of 10 000 reads rather than
/// at the next full collection.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader` with no `TapeSource::Lazy` borrow
/// of its tape live.
pub(crate) unsafe fn release_tape(hdr: *mut LazyArrayHeader) {
    if hdr.is_null() || (*hdr).tape.is_null() {
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

/// Publish a materialized element into its slot, with the owner barrier.
///
/// The slot is part of the owner allocation (it is the element slot the
/// materialized array keeps), so the ordinary in-object barrier applies: the
/// minor's dirty scan re-enters through the owner's `LazyArray` descriptor,
/// which visits the element range.
///
/// # Safety
///
/// `hdr` must be a live `LazyArrayHeader`, `i < cached_length`, and the slot
/// must still be a hole.
#[inline]
unsafe fn store_lazy_slot(hdr: *mut LazyArrayHeader, i: usize, value_bits: u64) {
    let slot = lazy_slot(hdr, i);
    debug_assert_eq!(*slot, crate::value::TAG_HOLE);
    // GC_STORE_AUDIT(BARRIERED): owner slot barrier below.
    slot.write(value_bits);
    (*hdr).cached_count += 1;
    crate::gc::runtime_write_barrier_slot(hdr as usize, slot as usize, value_bits);
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
/// 1. Element slot already materialized → return it.
/// 2. Cold read → walk the tape to the i-th entry via `link`
///    chasing, materialize that subtree, cache it, return.
///
/// Out-of-bounds returns `undefined`. Caller must ensure `hdr` is a
/// live LazyArrayHeader pointer; the materialize step uses the
/// arena allocator and may trigger GC (its `hdr` argument is
/// walked-through by the tracer if so, so the header survives).
#[inline(never)]
pub(crate) unsafe fn lazy_get_rooted_impl(hdr: *mut LazyArrayHeader, i: u32) -> JSValue {
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
    // The safepoint above can run a materialization (a test hook, or a
    // finalizer): the object may already be the ordinary array.
    if !is_lazy_array(hdr) {
        return crate::array::js_array_get(hdr as *const crate::array::ArrayHeader, i);
    }

    let cached_length = (*hdr).cached_length;
    if i >= cached_length {
        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Fast path: the element slot already holds the materialized value.
    if let Some(value) = lazy_cached_element(hdr, i) {
        return value;
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

    // Amortize the direct producer's shape metadata once consecutive reads
    // establish a traversal. Isolated reads keep the tape producer, which
    // avoids creating another parser cache for a glance at a few records.
    let value = (streak > 1)
        .then(|| record_materialize::try_small_record(&source, &scope, idx))
        .flatten()
        .unwrap_or_else(|| materialize_from_idx_source(&source, &scope, idx));
    let value_handle = scope.root_nanbox_u64(value.bits());
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    if !is_lazy_array(hdr) {
        // Materialized while this element was being produced: the array's own
        // slot is authoritative.
        return crate::array::js_array_get(hdr as *const crate::array::ArrayHeader, i);
    }
    if let Some(value) = lazy_cached_element(hdr, i) {
        // A nested read cached this element first; keep that identity.
        return value;
    }
    store_lazy_slot(hdr, i as usize, value_handle.get_nanbox_u64());

    // Adaptive thresholds. Either signal means future per-element walks
    // cost more than one full materialize, so trigger it now; afterwards
    // the object is an ordinary Array and no lazy read reaches it again.
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
    let flip = (*hdr).cumulative_walk_steps > (cached_length as u64) * 2 || scan_flip;
    crate::json_hooks::after_cold_read(hdr, flip, streak == cached_length);

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
) -> (bool, *mut LazyArrayHeader) {
    // The blob is this array's own source only when the tape root is the
    // blob's first value. Every production lazy array is built that way;
    // anything else declines rather than guessing.
    if (*hdr).root_idx != 0 {
        return (false, hdr);
    }
    let blob = (*hdr).blob_str;
    if blob.is_null() {
        return (false, hdr);
    }
    let blob_len = (*blob).byte_len as usize;
    if blob_len == 0 {
        return (false, hdr);
    }

    // Nothing between this read of `blob` and the suppression window can
    // collect, so the slice derived inside it names the live payload.
    let saved_roots = crate::json::parse_root_save_len();
    let (parsed_bits, hdr) = hdr_handle.across_mut::<LazyArrayHeader, _>(|| {
        let _suppress = crate::gc::GcSuppressScope::new();
        let data = (blob as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, blob_len);
        let mut parser = crate::json::DirectParser::new_batched(bytes);
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
        return (false, hdr);
    }

    let arr_addr = arr_ptr as usize;
    let (_, hdr) = hdr_handle.across_mut::<LazyArrayHeader, _>(|| {
        json_tape_safepoint(JsonTapeSafepoint::ForceLazyArrayRooted, arr_addr)
    });
    let arr_ptr = array_from_nanbox_handle(&arr_handle);

    // Fill the element slots from the reparsed array. A cached slot holds the
    // JSValue user code already has a reference to and may have MUTATED
    // through it, so the cache, not the reparsed subtree, is authoritative
    // there and its identity has to survive (`parsed[i] === parsed[i]`): only
    // holes are filled. The reparsed array is scaffolding and becomes garbage.
    //
    // This is a bulk fill of a dense array, so it follows the bulk rule
    // (`rebuild_array_layout`'s callers): raw word copies, then ONE layout
    // rebuild in `flip_to_array`, which classifies the slots and, for an old
    // owner, replays the range barrier (old-to-young remembering and mark
    // shading). A young owner owes no remembering; while a mark cycle is live
    // its new slots are shaded here as the per-slot barrier would. The
    // reparsed array is a fresh dense ARRAY of exactly `cached_length`
    // elements (checked above), and a JSON value is never a hole, so its
    // element words are the values themselves.
    //
    // The copy only stores, inside a nesting-safe suppression window, so
    // `hdr` / `arr_ptr` stay valid for its duration.
    {
        let _suppress = crate::gc::GcSuppressScope::new();
        let n = cached_length as usize;
        let src = crate::array::array_elements_ptr(arr_ptr as *const crate::array::ArrayHeader)
            as *const u64;
        let dst = lazy_slot(hdr, 0);
        let verbatim = lazy_cached_count(hdr) == 0;
        if verbatim {
            // GC_STORE_AUDIT(BARRIERED): bulk fill; flip_to_array rebuilds the
            // layout and replays the range barrier below.
            std::ptr::copy_nonoverlapping(src, dst, n);
        } else {
            for i in 0..n {
                let slot = dst.add(i);
                if *slot == crate::value::TAG_HOLE {
                    // GC_STORE_AUDIT(BARRIERED): bulk fill, as above.
                    slot.write(*src.add(i));
                }
            }
        }
        (*hdr).cached_count = cached_length;
        let owner = hdr as usize;
        // A verbatim copy of an all-pointer parse keeps the builder's proof.
        let all_pointers_proven = verbatim
            && n != 0
            && crate::array::array_object_flags_resolved(arr_ptr)
                & (crate::gc::GC_LAYOUT_STATE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS)
                == (crate::gc::GC_LAYOUT_SIDE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS);
        flip_to_array(hdr, all_pointers_proven);
        if !crate::arena::pointer_in_old_gen(owner) {
            // Old owners took the range barrier in `rebuild_array_layout`;
            // for a young owner this only shades, and only during a mark.
            crate::gc::replay_old_parent_slot_range_barriers(owner, dst, n);
        }
    }
    REPARSE_MATERIALIZATIONS.with(|c| c.set(c.get().wrapping_add(1)));
    (true, hdr)
}

/// Force-materialize a lazy array: fill every element slot that is still a
/// hole, then turn the object into the ordinary array in place
/// (`flip_to_array`). Returns the same address as an `ArrayHeader`.
/// Idempotent: on an already-flipped array it returns the array. Callers of
/// array accessors that don't have a lazy path invoke this first.
pub(crate) unsafe fn force_materialize_lazy_impl(
    hdr: *mut LazyArrayHeader,
) -> *mut crate::array::ArrayHeader {
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
    if !is_lazy_array(hdr) {
        // Already the ordinary array; it may have grown since (forwarding).
        return crate::array::clean_arr_ptr_mut(hdr as *mut crate::array::ArrayHeader);
    }
    let cached_length = (*hdr).cached_length;

    // #7499: with less than half the elements cached, one batch reparse of
    // the blob is cheaper than walking the tape for the rest.
    if lazy_cached_count(hdr) * 2 < cached_length as u64 {
        let (done, refreshed) = reparse_materialize(&scope, &hdr_handle, hdr, cached_length);
        if done {
            return refreshed as *mut crate::array::ArrayHeader;
        }
        hdr = refreshed;
    }

    // Element-wise: walk the tape once and materialize each hole into its
    // own slot. Cached slots are kept (identity).
    let source = TapeSource::Lazy { hdr_handle };
    let root = (*hdr).root_idx as usize;
    if let Some(root_entry) = source.entry(root) {
        if root_entry.kind == KIND_ARR_START {
            let end = root_entry.link as usize;
            let mut idx = root + 1;
            for i in 0..cached_length {
                if idx >= end {
                    break;
                }
                let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
                if lazy_cached_element(hdr, i).is_none() {
                    let elem_scope = crate::gc::RuntimeHandleScope::new();
                    let mut walk_idx = idx;
                    let value = materialize_value_source(&source, &elem_scope, &mut walk_idx);
                    let value_handle = elem_scope.root_nanbox_u64(value.bits());
                    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
                    if lazy_cached_element(hdr, i).is_none() {
                        store_lazy_slot(hdr, i as usize, value_handle.get_nanbox_u64());
                    }
                }
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
    }
    let hdr = hdr_handle.get_raw_mut_ptr::<LazyArrayHeader>();
    // A tape that ended early leaves trailing holes; they read as holes of
    // the array, never as stale words (`alloc_lazy_array_backing` wrote them).
    flip_to_array(hdr, false)
}

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

pub use crate::json_hooks::force_materialize_lazy;
use crate::json_hooks::lazy_get_rooted;
