//! `new TA(buffer, byteOffset, length?)` ArrayBuffer-view support (#4103).
//!
//! Split out of `typedarray.rs` to keep that file under the 2000-line gate.
//! Holds the spec `RangeError` bounds/alignment validation for the multi-arg
//! constructor form, plus the view-metadata side table (`TYPED_ARRAY_VIEW_META`)
//! that lets a typed array alias an `ArrayBuffer`: `data_ptr` (in
//! `typedarray::mod`) resolves a recorded view into the backing store so reads
//! and writes are shared with the buffer and every sibling view, and the
//! `byteOffset` / `buffer` getters report the real backing.

use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::typedarray::{
    clean_ta_ptr, data_ptr_mut, elem_size_for_kind, js_typed_array_new, name_for_kind,
    throw_range_error, typed_array_alloc, typed_array_to_array_buffer, TypedArrayHeader,
};

/// `ToIndex(value)` for a typed-array view's `byteOffset` / `length`
/// arguments (#4103): `undefined` / `NaN` → 0, otherwise `ToIntegerOrInfinity`
/// truncated toward zero, with a `RangeError` for a negative or
/// out-of-`[[0, 2^53-1]]` result.
pub(crate) fn typed_array_view_to_index(value: f64) -> i64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return 0;
    }
    // Real `ToNumber`: runs `valueOf`/`Symbol.toPrimitive` on objects
    // (observable, may throw) and throws TypeError on a Symbol or BigInt
    // argument — `jv.to_number()` did none of that, so an object byteOffset
    // silently became 0 and Symbol offsets/lengths never threw.
    let n = crate::typedarray::jsvalue_to_f64(value);
    if n.is_nan() {
        return 0;
    }
    // `ToIntegerOrInfinity` truncates BEFORE the range check, so a value in
    // (-1, 0) is 0, not a RangeError.
    let integer = n.trunc();
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        throw_range_error(b"Invalid typed array length");
    }
    integer as i64
}

/// `new TA(buffer, byteOffset, length?)` for the non-`Uint8Array` typed-array
/// kinds — create a view over an `ArrayBuffer` with the spec's offset/length
/// validation (#4103). Perry still models the result as an owning
/// `TypedArrayHeader` (the `byteOffset` getter reports 0), but the bounds
/// checks match Node and throw `RangeError` on violation:
///   - `byteOffset % BYTES_PER_ELEMENT == 0` (alignment),
///   - `byteOffset <= buffer.byteLength`,
///   - when `length` is omitted, the remaining bytes are a whole multiple of
///     `BYTES_PER_ELEMENT`,
///   - `byteOffset + length * BYTES_PER_ELEMENT <= buffer.byteLength`.
/// `offset_value` / `length_value` are the raw NaN-boxed arguments
/// (`undefined` when absent) so `ToIndex` runs here. A non-`ArrayBuffer`
/// source routes to the normal `js_typed_array_new` constructor.
#[no_mangle]
pub extern "C" fn js_typed_array_view(
    kind: i32,
    source: f64,
    offset_value: f64,
    length_value: f64,
) -> *mut TypedArrayHeader {
    let kind = kind as u8;
    let bits = source.to_bits();
    if (bits >> 48) != 0x7FFD {
        return js_typed_array_new(kind as i32, source);
    }
    let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if !crate::buffer::is_registered_buffer(addr) || !crate::buffer::is_any_array_buffer(addr) {
        return js_typed_array_new(kind as i32, source);
    }
    let bpe = elem_size_for_kind(kind) as i64;

    // ES ordering (InitializeTypedArrayFromArrayBuffer): ToIndex(byteOffset)
    // and ToIndex(length) run BEFORE the IsDetachedBuffer check — either can
    // execute user code (`valueOf`) that detaches the buffer — so all buffer
    // reads and bounds checks happen after, against post-coercion state.
    let offset = typed_array_view_to_index(offset_value);
    if bpe > 1 && offset % bpe != 0 {
        throw_range_error(
            format!(
                "start offset of {} should be a multiple of {}",
                name_for_kind(kind),
                bpe
            )
            .as_bytes(),
        );
    }
    let length_jv = crate::value::JSValue::from_bits(length_value.to_bits());
    let requested = if length_jv.is_undefined() {
        None
    } else {
        Some(typed_array_view_to_index(length_value))
    };
    if crate::buffer::is_detached_buffer(addr) {
        crate::typedarray::throw_type_error(b"Cannot perform Construct on a detached ArrayBuffer");
    }
    let src = addr as *const crate::buffer::BufferHeader;
    let total_len = unsafe { (*src).length as i64 };
    if offset > total_len {
        throw_range_error(
            format!("Start offset {offset} is outside the bounds of the buffer").as_bytes(),
        );
    }

    let elem_count = match requested {
        None => {
            let remaining = total_len - offset;
            if bpe > 1 && remaining % bpe != 0 {
                throw_range_error(
                    format!(
                        "byte length of {} should be a multiple of {}",
                        name_for_kind(kind),
                        bpe
                    )
                    .as_bytes(),
                );
            }
            remaining / bpe
        }
        Some(requested) => {
            if offset + requested * bpe > total_len {
                throw_range_error(format!("Invalid typed array length: {requested}").as_bytes());
            }
            requested
        }
    };

    let count = elem_count.max(0) as u32;
    let ta = typed_array_alloc(kind, count);
    if crate::buffer::is_shared_array_buffer(addr) {
        crate::typedarray::mark_typed_array_shared_backing(ta);
    }
    // Seed the header's inline region with the current bytes at `offset` so the
    // codegen fast path (which reads inline storage directly under optimized
    // builds) observes correct initial element values. `data_ptr_mut` resolves
    // to the inline region here because the view-meta below is not yet recorded.
    if count > 0 {
        unsafe {
            let src_data = crate::buffer::buffer_data(src).add(offset as usize);
            let dst = data_ptr_mut(ta);
            ptr::copy_nonoverlapping(src_data, dst, (count as i64 * bpe) as usize);
        }
    }
    // Record the view so the runtime element-access path aliases the backing
    // `ArrayBuffer` and `.byteOffset` / `.buffer` report the real backing:
    // mutations are then visible through the buffer and every sibling view,
    // matching Node (#4103).
    register_view_meta(ta, addr, offset as u32);
    ta
}

thread_local! {
    /// `typed_array_ptr -> (backing ArrayBuffer addr, byteOffset)` for typed
    /// arrays that alias an `ArrayBuffer`. Two populations land here:
    ///   * offset views built by `new T(buffer, byteOffset, length?)`, recorded
    ///     at construction so `.byteOffset` / `.buffer` report the real backing
    ///     and element reads/writes route through `data_ptr` into the shared
    ///     store (true aliasing), and
    ///   * plain typed arrays the first time `.buffer` is observed — we
    ///     lazily materialize a backing `ArrayBuffer`, copy the current bytes
    ///     in, and record it here so the buffer identity is stable on repeated
    ///     reads and further mutation aliases the buffer (mirrors V8: every
    ///     typed array is backed by an ArrayBuffer).
    /// The backing `BufferHeader` lives for the thread's lifetime (Perry never
    /// `dealloc`s individual buffers — see `buffer::view`), so the raw addr is
    /// stable and aliasing through it is free of use-after-free.
    static TYPED_ARRAY_VIEW_META: RefCell<crate::fast_hash::PtrHashMap<usize, ViewMeta>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Backing-store record for an ArrayBuffer-aliasing typed array. See
/// `TYPED_ARRAY_VIEW_META`.
#[derive(Copy, Clone)]
pub(crate) struct ViewMeta {
    /// Address of the backing `BufferHeader` (an `ArrayBuffer`).
    pub backing: usize,
    /// Byte offset of element 0 within `backing`.
    pub byte_offset: u32,
}

/// #5525: process-global count of typed arrays that have a `TYPED_ARRAY_VIEW_META`
/// entry (ArrayBuffer-aliasing or lazily-materialized `.buffer` views). The vast
/// majority of typed arrays are owning with inline storage and never appear
/// here, yet `view_backing_data_ptr` is on the hot per-element `data_ptr` path.
/// `any_view_meta` lets the common case skip the thread-local map probe with one
/// relaxed atomic load.
static VIEW_META_COUNT: AtomicUsize = AtomicUsize::new(0);

/// True if any typed array anywhere currently aliases an ArrayBuffer. When
/// false, no typed array can be a view, so element access can read inline
/// storage without consulting the (empty) view-meta side table.
#[inline]
pub(crate) fn any_view_meta() -> bool {
    VIEW_META_COUNT.load(Ordering::Relaxed) != 0
}

/// DetachArrayBuffer support (ES2024 `ArrayBuffer.prototype.transfer`): zero
/// the length of every typed array aliasing `backing` so views over a
/// detached buffer report length 0 and every indexed read is out-of-bounds
/// (`undefined`), matching Node.
pub(crate) fn zero_views_of_detached_backing(backing: usize) {
    if !any_view_meta() {
        return;
    }
    TYPED_ARRAY_VIEW_META.with(|r| {
        for (&ta, meta) in r.borrow().iter() {
            if meta.backing == backing {
                unsafe {
                    (*(ta as *mut TypedArrayHeader)).length = 0;
                }
            }
        }
    });
}

/// Record `ta` as aliasing `backing` at `byte_offset`. After this call
/// `data_ptr(ta)` resolves into the backing store rather than `ta`'s inline
/// region, so reads/writes are shared with the buffer and every other view.
pub(crate) fn register_view_meta(ta: *const TypedArrayHeader, backing: usize, byte_offset: u32) {
    TYPED_ARRAY_VIEW_META.with(|r| {
        let prev = r.borrow_mut().insert(
            ta as usize,
            ViewMeta {
                backing,
                byte_offset,
            },
        );
        if prev.is_none() {
            VIEW_META_COUNT.fetch_add(1, Ordering::Relaxed);
            // #5525 follow-up: this typed array now aliases an ArrayBuffer, so
            // its element-0 pointer no longer follows the header inline — bar
            // the codegen inline element fast path until it's gone.
            crate::typedarray::ta_view_guard_inc();
        }
    });
}

#[inline]
pub(crate) fn view_meta_of(addr: usize) -> Option<ViewMeta> {
    if !any_view_meta() {
        return None;
    }
    TYPED_ARRAY_VIEW_META.with(|r| r.borrow().get(&addr).copied())
}

/// Data pointer for element 0 of the typed array at `addr` when it aliases an
/// `ArrayBuffer` (resolves into the backing store at the recorded byteOffset);
/// `None` when the typed array uses its own inline storage. `data_ptr` /
/// `data_ptr_mut` in `typedarray::mod` consult this before falling back inline.
#[inline]
pub(crate) fn view_backing_data_ptr(addr: usize) -> Option<*mut u8> {
    view_meta_of(addr).map(|m| unsafe {
        crate::buffer::buffer_data_mut(m.backing as *mut crate::buffer::BufferHeader)
            .add(m.byte_offset as usize)
    })
}

/// A typed array's backing `ArrayBuffer` is reachable only through
/// `TYPED_ARRAY_VIEW_META`, as a raw address the collector cannot see. Without
/// this scanner the buffer is swept (or left stale after evacuation) while the
/// typed array still points at it, and `js_typed_array_backing_buffer` then
/// hands `js_typed_array_view` a dead pointer. That path does not fail loudly:
/// it falls back to `js_typed_array_new`, which reinterprets the dead buffer's
/// *byte* length as an element count — so `subarray(0, 11)` on an
/// `Int32Array(17)` returned a 68-element array (17 × 4 bytes) over a fresh
/// store, and the `set` that followed threw "offset is out of bounds".
///
/// Root the backing and let the visitor rewrite it, the way every other
/// object-keyed side table already does.
pub(crate) fn scan_typed_array_view_meta_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    if !any_view_meta() {
        return;
    }
    TYPED_ARRAY_VIEW_META.with(|r| {
        for meta in r.borrow_mut().values_mut() {
            let mut backing = meta.backing as *mut crate::buffer::BufferHeader;
            visitor.visit_raw_mut_ptr_slot(&mut backing);
            meta.backing = backing as usize;
        }
    });
}

/// Drop any recorded view metadata for `addr` (called from
/// `unregister_typed_array` when the typed array is collected).
pub(crate) fn clear_view_meta(addr: usize) {
    TYPED_ARRAY_VIEW_META.with(|r| {
        if r.borrow_mut().remove(&addr).is_some() {
            VIEW_META_COUNT.fetch_sub(1, Ordering::Relaxed);
            crate::typedarray::ta_view_guard_dec();
        }
    });
}

/// `%TypedArray%.prototype.byteOffset` for a registered typed array: the byte
/// offset of element 0 within its backing `ArrayBuffer`. Offset views recorded
/// in `TYPED_ARRAY_VIEW_META` report their real offset; everything else is 0.
pub fn js_typed_array_byte_offset(ta: *const TypedArrayHeader) -> u32 {
    let addr = clean_ta_ptr(ta) as usize;
    view_meta_of(addr).map(|m| m.byte_offset).unwrap_or(0)
}

/// `%TypedArray%.prototype.buffer` for a registered typed array: the backing
/// `ArrayBuffer`, with stable identity. If the typed array already aliases a
/// buffer (an offset view, or a previously-observed plain array) the recorded
/// backing is returned. Otherwise a backing `ArrayBuffer` is materialized once,
/// the current element bytes are copied in, and the typed array is rebound to
/// alias it so the identity stays stable and later mutation is shared — V8's
/// model where every typed array is backed by an ArrayBuffer.
pub fn js_typed_array_backing_buffer(
    ta: *const TypedArrayHeader,
) -> *mut crate::buffer::BufferHeader {
    let clean = clean_ta_ptr(ta);
    if clean.is_null() {
        return std::ptr::null_mut();
    }
    let addr = clean as usize;
    if let Some(meta) = view_meta_of(addr) {
        return meta.backing as *mut crate::buffer::BufferHeader;
    }
    // Materialize a stable backing ArrayBuffer over the current bytes, then
    // rebind the typed array to alias it (so `data_ptr` resolves into the
    // buffer from now on and writes are shared in both directions).
    let buf = typed_array_to_array_buffer(clean);
    if buf.is_null() {
        return std::ptr::null_mut();
    }
    register_view_meta(clean, buf as usize, 0);
    buf
}
