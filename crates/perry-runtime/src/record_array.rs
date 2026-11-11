//! Value-record arrays (value records, design §1.4): an array whose elements
//! are immutable records of immediates, stored inline at a stride of `words`
//! JSValue words.
//!
//! A record array is its own GC kind (`GC_TYPE_RECORD_ARRAY`), never an
//! ordinary array, and compiled code is the only thing that touches one, only
//! through the entries below. The head holds the counts, the template's shape
//! id and one traced edge: the raw pointer to a `GC_TYPE_RECORD_ELEMS` buffer
//! of `capacity * words` words. Every word is a canonical double or a
//! NaN-boxed immediate, so the buffer is a pointer-free leaf.
//!
//! `src` / `dst` always point into the caller's frame, never into the heap,
//! so they stay valid across the one allocation here (the growth in
//! [`js_record_array_push`]).

use crate::gc::{GC_TYPE_RECORD_ARRAY, GC_TYPE_RECORD_ELEMS};
use crate::value::{POINTER_MASK, TAG_TRUE, TAG_UNDEFINED};

#[cfg(test)]
mod tests;

/// The head of a record array. Indices `[0, length)` hold whole records.
#[repr(C)]
pub struct RecordArrayHeader {
    /// Records in use.
    pub length: u32,
    /// Records the buffer holds. Sits at payload `+4`, so it stays below the
    /// ShapeId range (`object::shape_rule3`); [`grown_capacity_or_throw`]
    /// bounds it.
    pub capacity: u32,
    /// JSValue words per record.
    pub words: u32,
    /// The record template's shape id (keys and order, for the readers).
    pub shape: u32,
    /// The element buffer, or null while `capacity` is 0 (and always for a
    /// record of no words).
    pub elems: *mut u64,
}

const _: () = assert!(std::mem::size_of::<RecordArrayHeader>() == 24);
const _: () = assert!(std::mem::offset_of!(RecordArrayHeader, capacity) == 4);
const _: () = assert!(std::mem::offset_of!(RecordArrayHeader, elems) == 16);

/// The first growth's capacity, in records.
const MIN_CAPACITY: u32 = 4;

/// The head's one traced edge, for the collector.
///
/// # Safety
/// `user_ptr` is the payload of a `GC_TYPE_RECORD_ARRAY` allocation.
#[inline]
pub(crate) unsafe fn elems_slot(user_ptr: *mut u8) -> *mut usize {
    std::ptr::addr_of_mut!((*user_ptr.cast::<RecordArrayHeader>()).elems).cast()
}

/// A value that is not a record array reached a record-array entry, or a
/// record array reached generic runtime code: the compiler's representation
/// facts were wrong. Reported like a check-mode false proof (exit code 86),
/// in every build mode.
#[cold]
#[inline(never)]
pub(crate) fn false_proof() -> ! {
    eprintln!("PERRY_FALSE_PROOF: value record array");
    // As `wpfacts_check::report`: leave without running exit handlers.
    unsafe { libc::_exit(86) }
}

/// Decode `arr`: a `POINTER_TAG` value whose GC header says record array.
/// Anything else is a compiler bug ([`false_proof`]).
#[inline(always)]
fn header_of(arr: f64) -> *mut RecordArrayHeader {
    let bits = arr.to_bits();
    match crate::value::addr_class::pointer_value_header(bits) {
        Some(header) if header.obj_type == GC_TYPE_RECORD_ARRAY => {
            (bits & POINTER_MASK) as *mut RecordArrayHeader
        }
        _ => false_proof(),
    }
}

/// The most records of `words` words one buffer holds: its allocation size
/// must fit `GcHeader::size`, and the count must stay below the ShapeId range.
#[inline]
fn max_capacity(words: u32) -> u32 {
    let payload = u32::MAX as usize - crate::gc::GC_HEADER_SIZE - 8;
    let fit = payload / (words.max(1) as usize * std::mem::size_of::<u64>());
    fit.min(crate::object::shape_rule3::MAX_PLUS_FOUR_WORD as usize) as u32
}

/// The capacity after one growth: `max(4, 2 * capacity)`, clamped to what one
/// buffer can hold. A full array at that limit raises
/// `RangeError: Invalid array length`, before any memory is reserved.
#[inline]
fn grown_capacity_or_throw(capacity: u32, words: u32) -> u32 {
    let max = max_capacity(words);
    if capacity >= max {
        crate::array::array_length_range_error()
    }
    capacity.saturating_mul(2).max(MIN_CAPACITY).min(max)
}

/// Grow a full array and return its head, which the allocation may have
/// moved.
///
/// # Safety
/// `arr` passed [`header_of`].
#[cold]
#[inline(never)]
unsafe fn grow(arr: f64) -> *mut RecordArrayHeader {
    let header = header_of(arr);
    let words = (*header).words;
    let capacity = grown_capacity_or_throw((*header).capacity, words);
    if words == 0 {
        // Records of no words need no buffer.
        (*header).capacity = capacity;
        return header;
    }
    let word_count = capacity as usize * words as usize;
    // The allocation can run a moving collection. The head is reachable from
    // Rust only through `arr`, so root it and take its address again
    // afterwards; the old buffer is read through the moved head's edge.
    let scope = crate::gc::RuntimeHandleScope::new();
    let root = scope.root_nanbox_f64(arr);
    let (buffer, arr) = root.across_nanbox(|| {
        crate::arena::arena_alloc_gc(
            word_count * std::mem::size_of::<u64>(),
            8,
            GC_TYPE_RECORD_ELEMS,
        )
        .cast::<u64>()
    });
    let header = (arr.to_bits() & POINTER_MASK) as *mut RecordArrayHeader;
    let live = (*header).length as usize * words as usize;
    if live != 0 {
        // GC_STORE_AUDIT(POINTER_FREE): record words are immediates, copied into a newborn leaf.
        std::ptr::copy_nonoverlapping((*header).elems, buffer, live);
    }
    // GC_STORE_AUDIT(BARRIERED): the head's one traced edge; an old head now
    // names a young buffer, and the barrier follows.
    (*header).elems = buffer;
    (*header).capacity = capacity;
    crate::gc::runtime_write_barrier_gc_slot(
        header as usize,
        std::ptr::addr_of!((*header).elems) as usize,
        buffer as u64,
    );
    header
}

/// A new, empty record array of `words`-word records of template `shape`, as
/// a `POINTER_TAG` JSValue. No buffer yet: the first push allocates it.
#[no_mangle]
pub extern "C" fn js_record_array_new(words: u32, shape: u32) -> f64 {
    let header = crate::arena::arena_alloc_gc(
        std::mem::size_of::<RecordArrayHeader>(),
        8,
        GC_TYPE_RECORD_ARRAY,
    )
    .cast::<RecordArrayHeader>();
    unsafe {
        // GC_STORE_AUDIT(INIT): birth; the buffer edge is born null.
        header.write(RecordArrayHeader {
            length: 0,
            capacity: 0,
            words,
            shape,
            elems: std::ptr::null_mut(),
        });
    }
    crate::value::js_nanbox_pointer(header as i64)
}

/// Append the record at `src` (`words` words in the caller's frame). Returns
/// the new length. May allocate, so the head may move.
#[no_mangle]
pub extern "C" fn js_record_array_push(arr: f64, src: *const u64) -> f64 {
    let mut header = header_of(arr);
    unsafe {
        if (*header).length == (*header).capacity {
            header = grow(arr);
        }
        let words = (*header).words as usize;
        let length = (*header).length;
        if words != 0 {
            // GC_STORE_AUDIT(POINTER_FREE): record words are immediates, stored into a leaf.
            std::ptr::copy_nonoverlapping(src, (*header).elems.add(length as usize * words), words);
        }
        (*header).length = length + 1;
        (length + 1) as f64
    }
}

/// Remove the last record. Empty: returns `undefined` and leaves `dst` alone.
/// Otherwise copies the record to `dst` and returns `true`.
#[no_mangle]
pub extern "C" fn js_record_array_pop(arr: f64, dst: *mut u64) -> f64 {
    let header = header_of(arr);
    unsafe {
        let length = (*header).length;
        if length == 0 {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let words = (*header).words as usize;
        if words != 0 {
            std::ptr::copy_nonoverlapping(
                (*header).elems.add((length - 1) as usize * words),
                dst,
                words,
            );
        }
        (*header).length = length - 1;
        f64::from_bits(TAG_TRUE)
    }
}

/// Read the record at `index`. An integer in `[0, length)` copies the record
/// to `dst` and returns `true`; any other number (negative, fractional, NaN,
/// out of bounds) returns `undefined` and leaves `dst` alone.
#[no_mangle]
pub extern "C" fn js_record_array_get(arr: f64, index: f64, dst: *mut u64) -> f64 {
    let header = header_of(arr);
    // A saturating conversion: NaN, a negative or a fractional index does not
    // convert back to itself, and a too large one is not below the length.
    let slot = index as u32;
    unsafe {
        if slot as f64 != index || slot >= (*header).length {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let words = (*header).words as usize;
        if words != 0 {
            std::ptr::copy_nonoverlapping((*header).elems.add(slot as usize * words), dst, words);
        }
        f64::from_bits(TAG_TRUE)
    }
}

/// The number of records.
#[no_mangle]
pub extern "C" fn js_record_array_length(arr: f64) -> f64 {
    unsafe { (*header_of(arr)).length as f64 }
}

/// `length = 0`. Keeps the buffer.
#[no_mangle]
pub extern "C" fn js_record_array_clear(arr: f64) {
    unsafe { (*header_of(arr)).length = 0 }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_RECORD_ARRAY_NEW: extern "C" fn(u32, u32) -> f64 = js_record_array_new;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_RECORD_ARRAY_PUSH: extern "C" fn(f64, *const u64) -> f64 = js_record_array_push;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_RECORD_ARRAY_POP: extern "C" fn(f64, *mut u64) -> f64 = js_record_array_pop;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_RECORD_ARRAY_GET: extern "C" fn(f64, f64, *mut u64) -> f64 = js_record_array_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_RECORD_ARRAY_LENGTH: extern "C" fn(f64) -> f64 = js_record_array_length;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_RECORD_ARRAY_CLEAR: extern "C" fn(f64) = js_record_array_clear;
