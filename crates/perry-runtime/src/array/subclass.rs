//! `class X extends Array` support — predicates + a dense-snapshot materializer.
//!
//! An Array subclass instance is a plain `ObjectHeader` (perry has no exotic
//! array-object representation), so its inherited `Array.prototype` methods run
//! through the spec-generic array-like engine in [`super::generic`], and
//! iteration / spread materialize a dense snapshot of its indexed elements.
//! Kept out of `generic.rs` so that module stays under the file-size gate.

use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use super::generic::{al_get, al_length, nanbox_arr};
use crate::array::{js_array_alloc_with_length, note_array_slot, ArrayHeader};
use crate::object::ObjectHeader;
use crate::value::JSValue;

// #8690: `ObjectMeta::flags` carries the move-stable scalar payload for a
// numeric packed-prefix proof. The GcHeader authority bit prevents a record
// surviving address reuse: fresh allocations have it clear, and both words
// ride an evacuation without a side-table re-key walk.
//
//     bit 0       existing custom-[[Prototype]] flag
//     bit 1       payload valid
//     bits 8..31  verified numeric prefix bound (24 bits, max 16,000,000)
//     bits 32..63 exact semantic ShapeId
const PACKED_NUMERIC_META_VALID: u64 = 1 << 1;
const PACKED_NUMERIC_META_BOUND_SHIFT: u32 = 8;
const PACKED_NUMERIC_META_BOUND_MASK: u64 = 0x00FF_FFFF << PACKED_NUMERIC_META_BOUND_SHIFT;
const PACKED_NUMERIC_META_SHAPE_MASK: u64 = 0xFFFF_FFFF_0000_0000;
const PACKED_NUMERIC_META_MASK: u64 =
    PACKED_NUMERIC_META_VALID | PACKED_NUMERIC_META_BOUND_MASK | PACKED_NUMERIC_META_SHAPE_MASK;

// #8655: Array-subclass instances use ordinary ObjectHeader property slots,
// but their hot numeric reads have a much stronger invariant than a generic
// object lookup can exploit: `push` appends the own keys `"0"`, `"1"`, ... in
// order, and every structural/descriptor/prototype mutation publishes a new
// ShapeId before it becomes observable. Cache that dense prefix per exact
// (class, shape) pair so a stable `sub[i]` is two field-slot reads (`length`
// and the element) instead of number -> String allocation + hash lookup.
//
// The cache stores no heap pointer, so it is not a GC root. ShapeIds are never
// reused, and the class id prevents an unrelated class with the same ordered
// keys from borrowing the Array-subclass proof.
const DENSE_SUBCLASS_CACHE_SLOTS: usize = 256;

struct DenseSubclassCacheEntry {
    /// Even while stable, odd while a colliding writer publishes a payload.
    sequence: AtomicU64,
    /// `(class_id << 32) | shape_id`.
    key: AtomicU64,
    /// `(length_slot << 32) | element_base`.
    slots: AtomicU64,
    /// `(live_inline_slots << 32) | dense_prefix_len`.
    bounds: AtomicU64,
}

impl DenseSubclassCacheEntry {
    const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            key: AtomicU64::new(0),
            slots: AtomicU64::new(0),
            bounds: AtomicU64::new(0),
        }
    }
}

static DENSE_SUBCLASS_CACHE: [DenseSubclassCacheEntry; DENSE_SUBCLASS_CACHE_SLOTS] =
    [const { DenseSubclassCacheEntry::new() }; DENSE_SUBCLASS_CACHE_SLOTS];

#[derive(Clone, Copy)]
struct DenseSubclassLayout {
    length_slot: u32,
    element_base: u32,
    dense_prefix_len: u32,
    live_inline_slots: u32,
}

#[inline(always)]
fn dense_cache_key(class_id: u32, shape_id: u32) -> u64 {
    ((class_id as u64) << 32) | shape_id as u64
}

#[inline(always)]
fn dense_cache_entry(key: u64) -> &'static DenseSubclassCacheEntry {
    let mixed = key ^ (key >> 33) ^ (key >> 17);
    &DENSE_SUBCLASS_CACHE[mixed as usize & (DENSE_SUBCLASS_CACHE_SLOTS - 1)]
}

#[inline]
fn cached_dense_layout(key: u64) -> Option<DenseSubclassLayout> {
    let entry = dense_cache_entry(key);
    let sequence = entry.sequence.load(Ordering::Acquire);
    if sequence & 1 != 0 || entry.key.load(Ordering::Relaxed) != key {
        return None;
    }
    let slots = entry.slots.load(Ordering::Relaxed);
    let bounds = entry.bounds.load(Ordering::Relaxed);
    // Recheck the seqlock before interpreting either word so readers never
    // combine payloads from two colliding publishers. The fence stops the
    // relaxed payload loads above from being reordered past this recheck on
    // weakly ordered targets - an Acquire load alone only orders what comes
    // after it, not what precedes it in program order.
    std::sync::atomic::fence(Ordering::Acquire);
    if entry.sequence.load(Ordering::Acquire) != sequence {
        return None;
    }
    Some(DenseSubclassLayout {
        length_slot: (slots >> 32) as u32,
        element_base: slots as u32,
        dense_prefix_len: bounds as u32,
        live_inline_slots: (bounds >> 32) as u32,
    })
}

#[inline]
fn publish_dense_layout(key: u64, layout: DenseSubclassLayout) {
    let entry = dense_cache_entry(key);
    let mut sequence = entry.sequence.load(Ordering::Relaxed);
    loop {
        if sequence & 1 != 0 {
            std::hint::spin_loop();
            sequence = entry.sequence.load(Ordering::Relaxed);
            continue;
        }
        match entry.sequence.compare_exchange_weak(
            sequence,
            sequence.wrapping_add(1),
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => sequence = observed,
        }
    }
    entry.slots.store(
        ((layout.length_slot as u64) << 32) | layout.element_base as u64,
        Ordering::Relaxed,
    );
    entry.bounds.store(
        ((layout.live_inline_slots as u64) << 32) | layout.dense_prefix_len as u64,
        Ordering::Relaxed,
    );
    entry.key.store(key, Ordering::Relaxed);
    entry
        .sequence
        .store(sequence.wrapping_add(2), Ordering::Release);
}

fn decimal_u32<'a>(mut value: u32, buf: &'a mut [u8; 10]) -> &'a [u8] {
    let mut start = buf.len();
    loop {
        start -= 1;
        buf[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            return &buf[start..];
        }
    }
}

/// Establish the dense-prefix invariant once for an exact semantic ShapeId.
/// This path may scan the keys array, but it runs only on a cache miss. It
/// allocates nothing and keeps no address into the moving heap.
unsafe fn build_dense_layout(obj: *const ObjectHeader) -> Option<DenseSubclassLayout> {
    let class_id = (*obj).class_id;
    if class_id == 0
        || !is_array_subclass_class_id(class_id)
        || crate::object::prototype_chain::object_has_prototype_override(obj as usize)
    {
        return None;
    }
    let shape = crate::object::shapes::object_shape_descriptor(obj)?;
    if shape.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return None;
    }
    let (key_slots, physical_len) = crate::object::keys_array_dense_slots(keys);
    let key_count = (shape.logical_key_count as usize).min(physical_len);
    if key_slots.is_null() || key_count == 0 {
        return None;
    }

    let mut length_slot = None;
    let mut element_base = None;
    for slot in 0..key_count {
        let stored = JSValue::from_bits((*key_slots.add(slot)).to_bits());
        if length_slot.is_none() && crate::string::js_string_key_matches_bytes(stored, b"length") {
            length_slot = Some(slot as u32);
        }
        if element_base.is_none() && crate::string::js_string_key_matches_bytes(stored, b"0") {
            element_base = Some(slot as u32);
        }
    }
    let length_slot = length_slot?;
    // A length-only empty subclass has no `"0"` key yet. Cache its length
    // read, while leaving the numeric prefix empty so every index side-exits.
    let has_element_zero = element_base.is_some();
    let element_base = element_base.unwrap_or(0);
    let mut dense_prefix_len = 0u32;
    if has_element_zero {
        while (element_base as usize + dense_prefix_len as usize) < key_count {
            let slot = element_base as usize + dense_prefix_len as usize;
            let stored = JSValue::from_bits((*key_slots.add(slot)).to_bits());
            let mut decimal = [0u8; 10];
            if !crate::string::js_string_key_matches_bytes(
                stored,
                decimal_u32(dense_prefix_len, &mut decimal),
            ) {
                break;
            }
            dense_prefix_len += 1;
        }
    }

    // Class construction installs descriptors for unrelated methods, so the
    // object-wide descriptor bit is too coarse for this proof. Data
    // descriptors do not alter [[Get]]; reject only accessors for the slots the
    // fast path will read. Descriptor mutations publish a new semantic
    // ShapeId, which makes this one-time scan part of the exact-shape proof.
    if crate::object::object_has_descriptors(obj as usize) {
        if crate::object::get_accessor_descriptor(obj as usize, "length").is_some() {
            return None;
        }
        for index in 0..dense_prefix_len {
            let mut decimal = [0u8; 10];
            let bytes = decimal_u32(index, &mut decimal);
            // `decimal_u32` emits ASCII digits only.
            let key = unsafe { std::str::from_utf8_unchecked(bytes) };
            if crate::object::get_accessor_descriptor(obj as usize, key).is_some() {
                return None;
            }
        }
    }

    Some(DenseSubclassLayout {
        length_slot,
        element_base,
        dense_prefix_len,
        live_inline_slots: shape.live_inline_slot_count,
    })
}

/// Resolve a live Array-subclass object and its cached dense layout. Every
/// rejected brand, forwarding, descriptor, hole, or prototype case returns
/// `None`; callers retain their existing fully generic fallback.
#[inline]
fn dense_layout_for_value(value: f64) -> Option<(*const ObjectHeader, DenseSubclassLayout)> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let obj = js.as_pointer::<ObjectHeader>();
    if obj.is_null() || !crate::object::is_valid_obj_ptr(obj.cast::<u8>()) {
        return None;
    }
    let header = unsafe { crate::value::addr_class::try_read_gc_header(obj as usize)? };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    // This is per receiver, not per ShapeId. A cached layout built before
    // Object.setPrototypeOf must not let this object borrow the old proof.
    if crate::object::prototype_chain::object_has_prototype_override(obj as usize) {
        return None;
    }
    let (class_id, shape_id) = unsafe { ((*obj).class_id, (*obj).parent_class_id) };
    let key = dense_cache_key(class_id, shape_id);
    let layout = cached_dense_layout(key).or_else(|| {
        let layout = unsafe { build_dense_layout(obj) }?;
        publish_dense_layout(key, layout);
        Some(layout)
    })?;
    Some((obj, layout))
}

/// Clear an established Array-subclass numeric-prefix proof before an owner
/// field store. `layout_note_slot` calls this for inline slots; the object-owned
/// spill path calls it against the owner because its physical store is noted
/// on the child Array buffer instead.
#[inline]
pub(crate) unsafe fn clear_packed_subclass_numeric_proof(obj: *mut ObjectHeader) {
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF == 0
    {
        return;
    }
    let header = std::ptr::from_ref(header).cast_mut();
    // Retire the authority first. A missing/moving meta then merely leaves an
    // unreachable payload, never a proof a future query can consume.
    (*header)._reserved &= !crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
    let meta = (*obj).meta;
    if !meta.is_null() {
        (*meta).flags &= !PACKED_NUMERIC_META_MASK;
    }
}

/// Owner-side invalidation for an object-owned spill write. The common
/// no-proof case uses the meta pointer the spill path already loaded and pays
/// only one predictable bit test; it does not re-read the owner's GC header.
#[inline]
pub(crate) unsafe fn note_packed_subclass_spill_store(
    obj: *mut ObjectHeader,
    meta: *mut crate::object::ObjectMeta,
) {
    if !meta.is_null() && (*meta).flags & PACKED_NUMERIC_META_VALID != 0 {
        clear_packed_subclass_numeric_proof(obj);
    }
}

#[inline]
unsafe fn subclass_numeric_prefix_is_proven(
    obj: *const ObjectHeader,
    shape_id: u32,
    bound: u32,
) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return false;
    };
    let header = std::ptr::from_ref(header).cast_mut();
    if (*header)._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF == 0 {
        return false;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        (*header)._reserved &= !crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
        return false;
    }
    let flags = (*meta).flags;
    let payload_valid = flags & PACKED_NUMERIC_META_VALID != 0;
    let proven_bound =
        ((flags & PACKED_NUMERIC_META_BOUND_MASK) >> PACKED_NUMERIC_META_BOUND_SHIFT) as u32;
    let proven_shape = (flags >> 32) as u32;
    if payload_valid && proven_shape == shape_id && proven_bound >= bound {
        return true;
    }
    clear_packed_subclass_numeric_proof(obj as *mut ObjectHeader);
    false
}

#[inline]
unsafe fn publish_subclass_numeric_prefix(
    obj: *const ObjectHeader,
    shape_id: u32,
    bound: u32,
) -> bool {
    let meta = (*obj).meta;
    if meta.is_null() || bound > 16_000_000 {
        return false;
    }
    let flags = (*meta).flags;
    (*meta).flags = (flags & !PACKED_NUMERIC_META_MASK)
        | PACKED_NUMERIC_META_VALID
        | (u64::from(bound) << PACKED_NUMERIC_META_BOUND_SHIFT)
        | (u64::from(shape_id) << 32);
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return false;
    };
    let header = std::ptr::from_ref(header).cast_mut();
    (*header)._reserved |= crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
    true
}

/// Establish-or-confirm the numeric prefix used by the call-free loop clone.
/// The first visit scans; later visits are two scalar-word checks. Any owner
/// store retires the record before writing, and semantic shape changes fail
/// the exact ShapeId comparison even if they do not touch a value slot.
#[inline]
unsafe fn ensure_subclass_numeric_prefix(
    obj: *const ObjectHeader,
    layout: DenseSubclassLayout,
    bound: u32,
) -> bool {
    if bound == 0 {
        return true;
    }
    let shape_id = (*obj).parent_class_id;
    if subclass_numeric_prefix_is_proven(obj, shape_id, bound) {
        return true;
    }
    for index in 0..bound {
        let Some(slot) = layout.element_base.checked_add(index) else {
            return false;
        };
        let value_ptr = if slot < layout.live_inline_slots {
            (obj as *mut u8)
                .add(std::mem::size_of::<ObjectHeader>())
                .cast::<u64>()
                .add(slot as usize)
        } else {
            let meta = (*obj).meta;
            if meta.is_null() {
                return false;
            }
            let spill = (*meta).spill as *mut ArrayHeader;
            if spill.is_null() || slot >= (*spill).length {
                return false;
            }
            (spill as *mut u8)
                .add(std::mem::size_of::<ArrayHeader>())
                .cast::<u64>()
                .add(slot as usize)
        };
        let value = JSValue::from_bits(ptr::read(value_ptr));
        if value.is_int32() {
            // `push(i)` commonly stores Perry's compact INT32 Number tag. The
            // direct clone consumes raw doubles, so normalize that Number to
            // its representation-equivalent f64 bits during the one-time
            // verification walk. This is pointer-free -> pointer-free and
            // changes no JS-observable type/value, hence needs neither a GC
            // barrier nor a layout downgrade.
            // GC_STORE_AUDIT(POINTER_FREE): canonical raw-f64 Number bits
            // replace compact int32 Number bits in an already numeric slot.
            ptr::write(value_ptr, (value.as_int32() as f64).to_bits());
        } else if !value.is_number() {
            return false;
        }
    }
    publish_subclass_numeric_prefix(obj, shape_id, bound)
}

#[inline]
fn layout_length_value(obj: *const ObjectHeader, layout: DenseSubclassLayout) -> JSValue {
    layout_field_value(obj, layout.length_slot, layout.live_inline_slots)
}

/// Read a slot already proved live by an exact ShapeId. Wide dynamic objects
/// keep post-inline fields in the object-owned spill Array. Reaching that
/// buffer directly is the essential #8655 hot path: the general field helper
/// reclassifies the owner and probes the overflow abstraction for every ECS
/// element even though the shape proof already established all of it.
#[inline(always)]
fn layout_field_value(obj: *const ObjectHeader, slot: u32, live_inline_slots: u32) -> JSValue {
    unsafe {
        if slot < live_inline_slots {
            let fields =
                (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
            return *fields.add(slot as usize);
        }

        if crate::object::object_spill_enabled() {
            let meta = (*obj).meta;
            if !meta.is_null() {
                let spill = (*meta).spill as *const ArrayHeader;
                if !spill.is_null() && slot < (*spill).length {
                    let elements =
                        (spill as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
                    return JSValue::from_bits(*elements.add(slot as usize));
                }
            }
            return JSValue::undefined();
        }

        crate::object::overflow_get(obj as usize, slot as usize)
            .map(JSValue::from_bits)
            .unwrap_or_else(JSValue::undefined)
    }
}

fn nonnegative_u32_length(value: JSValue) -> Option<u32> {
    let number = if value.is_int32() {
        value.as_int32() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        return None;
    };
    (number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= u32::MAX as f64)
        .then_some(number as u32)
}

/// Fast own `length` read for an object-backed Array subclass. Returning the
/// stored JSValue (rather than coercing it) preserves source property-read
/// semantics; descriptor/prototype-divergent shapes decline above.
#[inline]
pub(crate) fn array_subclass_fast_length(value: f64) -> Option<f64> {
    let (obj, layout) = dense_layout_for_value(value)?;
    Some(f64::from_bits(layout_length_value(obj, layout).bits()))
}

/// Guarded dense numeric read for an object-backed Array subclass. The live
/// `length` value is checked on every hit, while `dense_prefix_len` caps the
/// proof when a length-only grow created holes without changing the shape.
#[inline]
pub(crate) fn array_subclass_fast_index_get(value: f64, index: u32) -> Option<f64> {
    let (obj, layout) = dense_layout_for_value(value)?;
    dense_index_get_with_layout(obj, layout, index)
}

#[inline(always)]
fn dense_index_get_with_layout(
    obj: *const ObjectHeader,
    layout: DenseSubclassLayout,
    index: u32,
) -> Option<f64> {
    let length = nonnegative_u32_length(layout_length_value(obj, layout))?;
    if index >= length || index >= layout.dense_prefix_len {
        return None;
    }
    let slot = layout.element_base.checked_add(index)?;
    let value = layout_field_value(obj, slot, layout.live_inline_slots);
    Some(f64::from_bits(value.bits()))
}

fn canonical_u32_index(value: f64) -> Option<u32> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        return (js.as_int32() >= 0).then_some(js.as_int32() as u32);
    }
    (js.is_number()
        && value.is_finite()
        && value >= 0.0
        && value.fract() == 0.0
        && value <= (u32::MAX - 1) as f64)
        .then_some(value as u32)
}

/// Unknown-receiver numeric read used by codegen's guarded typed-array miss
/// block. Stable real arrays and Array subclasses terminate here; every other
/// receiver/key keeps the established tag-aware dispatcher as a cold side
/// exit. Keeping that call behind this ABI boundary removes `js_dyn_index_get`
/// from the emitted hot-loop artifact without weakening its semantics.
#[no_mangle]
/// The five optional IC words are
/// scalar layout facts, never heap pointers:
/// `(class_id, ShapeId)`, length slot, element base, dense prefix, inline bound.
/// The emitted hit path reloads the live object/meta/spill pointers, so moving
/// GC never has to trace or rewrite this cache.
pub extern "C" fn js_packed_arraylike_index_get(receiver: f64, index: f64, cache: *mut u64) -> f64 {
    if let Some(index_u32) = canonical_u32_index(index) {
        let js = JSValue::from_bits(receiver.to_bits());
        if js.is_pointer() {
            let raw = js.as_pointer::<u8>();
            if let Some(header) =
                unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) }
            {
                if matches!(
                    header.obj_type,
                    crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
                ) {
                    return crate::array::js_array_get_f64(
                        raw as *const crate::array::ArrayHeader,
                        index_u32,
                    );
                }
                if header.obj_type == crate::gc::GC_TYPE_OBJECT {
                    if let Some((obj, layout)) = dense_layout_for_value(receiver) {
                        // The codegen hit path reads length inline and wide
                        // slots through ObjectMeta::spill. Decline to prime in
                        // the legacy side-table mode or for a pathological
                        // layout whose length itself spilled.
                        if !cache.is_null()
                            && crate::object::object_spill_enabled()
                            && layout.length_slot < layout.live_inline_slots
                        {
                            unsafe {
                                cache.add(1).write(layout.length_slot as u64);
                                cache.add(2).write(layout.element_base as u64);
                                cache.add(3).write(layout.dense_prefix_len as u64);
                                cache.add(4).write(layout.live_inline_slots as u64);
                                cache.write(dense_cache_key(
                                    (*obj).class_id,
                                    (*obj).parent_class_id,
                                ));
                            }
                        }
                        if let Some(value) = dense_index_get_with_layout(obj, layout, index_u32) {
                            return value;
                        }
                    }
                }
            }
        }
    }
    crate::value::js_dyn_index_get(receiver, index)
}

/// Admit a complete counted-loop range over either an ordinary Array or an
/// object-backed Array subclass. The seven output words are scalar facts, not
/// managed pointers, so the generated loop can reload a relocated receiver
/// from its root before each residual check.
///
/// Layout: `(kind, gc_header, receiver_header, length_slot, element_base,
/// dense_prefix|inline_bound<<32, bound)`. Kind 1 is an ArrayHeader and kind 2
/// is an ObjectHeader Array subclass. A zero return leaves every semantic case
/// to the unchanged generic loop.
#[no_mangle]
pub extern "C" fn js_packed_arraylike_loop_guard(
    receiver: f64,
    bound: f64,
    require_numeric: i32,
    out: *mut u64,
) -> i32 {
    let live_length_bound = bound == -1.0;
    if out.is_null()
        || !bound.is_finite()
        || (!live_length_bound && bound < 0.0)
        || (!live_length_bound && bound.fract() != 0.0)
        || bound > 16_000_000.0
    {
        return 0;
    }
    let requested_bound = (!live_length_bound).then_some(bound as u32);
    let js = JSValue::from_bits(receiver.to_bits());
    if !js.is_pointer() {
        return 0;
    }
    let raw = js.as_pointer::<u8>();
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) })
    else {
        return 0;
    };
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return 0;
    }

    if header.obj_type == crate::gc::GC_TYPE_ARRAY {
        if header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
            || super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) != 0
        {
            return 0;
        }
        let array = raw.cast::<ArrayHeader>();
        let (length, capacity) = unsafe { ((*array).length, (*array).capacity) };
        let bound = requested_bound.unwrap_or(length);
        if bound > length || length > capacity || capacity > 16_000_000 {
            return 0;
        }
        if require_numeric != 0 {
            // The raw-f64 invariant is an O(1) GcHeader bit after its first
            // self-healing scan, and every nonnumeric Array write already
            // clears it. Reuse that representation proof instead of walking
            // the full range on every invocation of the surrounding scan().
            if !unsafe { super::header::ensure_array_numeric_raw_f64(array as *mut ArrayHeader) } {
                return 0;
            }
        }
        let gc_word = unsafe { ptr::read_unaligned((raw as *const u8).sub(8).cast::<u64>()) };
        let array_word = (u64::from(capacity) << 32) | u64::from(length);
        unsafe {
            out.add(0).write(1);
            out.add(1).write(gc_word);
            out.add(2).write(array_word);
            out.add(3).write(0);
            out.add(4).write(0);
            out.add(5).write(0);
            out.add(6).write(u64::from(bound));
        }
        return 1;
    }

    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return 0;
    }
    let Some((object, layout)) = dense_layout_for_value(receiver) else {
        return 0;
    };
    if !crate::object::object_spill_enabled() || layout.length_slot >= layout.live_inline_slots {
        return 0;
    }
    let Some(length) = nonnegative_u32_length(layout_length_value(object, layout)) else {
        return 0;
    };
    let bound = requested_bound.unwrap_or(length);
    if bound > length || bound > layout.dense_prefix_len || length > 16_000_000 {
        return 0;
    }
    if require_numeric != 0 {
        if !unsafe { ensure_subclass_numeric_prefix(object, layout, bound) } {
            return 0;
        }
    }
    let gc_word = unsafe { ptr::read_unaligned((raw as *const u8).sub(8).cast::<u64>()) };
    let receiver_word = unsafe { ptr::read_unaligned(raw.cast::<u64>()) };
    unsafe {
        out.add(0).write(2);
        out.add(1).write(gc_word);
        out.add(2).write(receiver_word);
        out.add(3).write(u64::from(layout.length_slot));
        out.add(4).write(u64::from(layout.element_base));
        out.add(5).write(
            (u64::from(layout.live_inline_slots) << 32) | u64::from(layout.dense_prefix_len),
        );
        out.add(6).write(u64::from(bound));
    }
    2
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_LOOP_GUARD: extern "C" fn(f64, f64, i32, *mut u64) -> i32 =
    js_packed_arraylike_loop_guard;

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_INDEX_GET: extern "C" fn(f64, f64, *mut u64) -> f64 =
    js_packed_arraylike_index_get;

/// True when `class_id` is a user class that extends `Array` (the reserved
/// parent id `0xFFFF0024` appears in its class chain), i.e. `class X extends
/// Array`. Such instances are plain `ObjectHeader`s, so the array-like engines
/// must run on them (`x.push(1)`, `x.map(...)`) — they are otherwise excluded by
/// the "plain objects only" guard alongside ordinary user classes. Purely
/// additive: only newly admits Array subclasses, never changes plain-object or
/// ordinary-class-instance behavior.
pub(crate) fn is_array_subclass_class_id(class_id: u32) -> bool {
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == 0 {
        return false;
    }
    let mut cur = class_id;
    // Bounded walk up the parent chain; guards against a corrupt cyclic edge.
    for _ in 0..64 {
        match crate::object::get_parent_class_id(cur) {
            Some(parent) if parent == CLASS_ID_ARRAY => return true,
            Some(parent) => cur = parent,
            None => return false,
        }
    }
    false
}

/// True when `object` is a live `class X extends Array` instance: a heap
/// `GC_TYPE_OBJECT` whose class id chains to the reserved `Array` parent id.
/// Used to route inherited *read* Array methods (`map` / `filter` / `join` /
/// `at` / `indexOf` / …) and iteration/spread over the subclass instance.
/// `try_read_gc_header` magnitude-classifies the address first, so a non-heap
/// handle id is never dereferenced as a `GcHeader`.
pub fn is_array_subclass_instance(object: f64) -> bool {
    let jsv = JSValue::from_bits(object.to_bits());
    if !jsv.is_pointer() {
        return false;
    }
    let raw = jsv.as_pointer::<u8>();
    if raw.is_null() || !crate::object::is_valid_obj_ptr(raw) {
        return false;
    }
    let obj_type = match unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) } {
        Some(hdr) => hdr.obj_type,
        None => return false,
    };
    if obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let class_id = crate::object::js_object_get_class_id(raw as *const ObjectHeader);
    is_array_subclass_class_id(class_id)
}

/// Materialize a `class X extends Array` instance into a fresh dense array by
/// reading its `length` + indexed elements through the array-like accessors.
/// Iteration (`for…of`, spread, `Array.from`, destructuring, `[].concat(sub)`)
/// drives the array iterator / spread, which read a real `ArrayHeader`; an
/// object-backed subclass instance would be misread, so those paths iterate
/// this snapshot instead. Snapshot (not live) semantics — a full fix would need
/// an object-backed array iterator. Absent indices materialize as `undefined`
/// (not preserved holes): correct for iteration/spread (the array iterator
/// yields `undefined` for holes anyway); a sparse subclass fed to `concat`
/// therefore yields `undefined` rather than a preserved hole — an accepted
/// limitation for this rare case.
pub fn array_subclass_dense_snapshot(recv: f64) -> f64 {
    let len = al_length(recv).max(0);
    // ArrayCreate throws a RangeError for len ≥ 2^32 (matching `js_arraylike_map`)
    // — and, critically, this guard prevents the `as u32` truncation below from
    // under-allocating the buffer while the `0..len` loop iterates the full i64
    // count and writes out of bounds.
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let result = js_array_alloc_with_length(len as u32);
    let elems = unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..len {
        let v = al_get(recv, k);
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): note_array_slot re-stores with the barrier.
            ptr::write(elems.add(k as usize), v);
            note_array_slot(result, k as usize, v.to_bits());
        }
    }
    nanbox_arr(result)
}

/// True when an Array-subclass instance carries a USER `[Symbol.iterator]`
/// override — an own `inst[Symbol.iterator] = …` / symbol accessor, or a class
/// method `*[Symbol.iterator]()` (registered under the synthetic `@@iterator`
/// name). The default array iterator is a runtime default (not a class vtable
/// method), so a hit means the user declared their own. The snapshot iteration
/// shortcuts must defer to such an override and only synthesize the default
/// array iterator when none exists. Mirrors
/// `object::map_set_subclass::subclass_has_iterator_override`.
pub fn array_subclass_has_iterator_override(value: f64) -> bool {
    let iter_wk = crate::symbol::well_known_symbol("iterator");
    if iter_wk.is_null() {
        return false;
    }
    let iter_f64 = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
    if unsafe { crate::symbol::own_symbol_property(value, iter_f64) }.is_some() {
        return true;
    }
    let raw = value.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    let class_id = crate::object::js_object_get_class_id(raw as *const ObjectHeader);
    class_id != 0 && crate::object::method_owner_class_id(class_id, "@@iterator").is_some()
}

// ---------------------------------------------------------------------------
// #7574 — raw `js_array_*` receiver resolution for an array-like OBJECT.
//
// Codegen decides "this receiver is an Array" from the DECLARED TypeScript type
// of the binding (`is_array_expr` / `Type::Array(_)` / `Generic { base: "Array" }`),
// then emits a raw `js_array_*` call whose first act is to dereference the
// receiver as an `ArrayHeader`. A declared type is a hint, never a layout fact
// (CLAUDE.md, *Known Limitations*: annotations are erased and nothing validates
// them at runtime), so any binding annotated with the BASE type — `const a:
// number[] = new MyArr()`, a parameter, a class field, a return type, an
// `as number[]` cast — can be holding a `class X extends Array` instance, which
// perry models as a plain `ObjectHeader`. The two headers overlay field for
// field:
//
//     ArrayHeader.length   (u32 @0)  <- ObjectHeader.class_id        (#8113)
//     ArrayHeader.capacity (u32 @4)  <- ObjectHeader.parent_class_id  (ShapeId)
//     elements[0]          (@8)      <- keys_array   (a *mut ArrayHeader)
//     elements[1]          (@16)     <- meta         (a *mut ObjectMeta)
//     elements[2]          (@24)     <- inline field slot 0
//
// so element WRITES overwrite two live GC child edges with arbitrary doubles —
// the collector then traces whatever the mutator stored. `a.push(1); a.push(2)`
// SIGSEGVs (exit 139) on the second push.
//
// `clean_arr_ptr` now refuses a `GC_TYPE_OBJECT` allocation outright, which
// makes every one of its ~190 call sites fail-CLOSED. That is the memory-safety
// half. The correctness half is these helpers: the entry points reachable from
// the declared-type codegen tiers re-enter through their EXISTING null branch
// and run the operation on the spec-generic array-like engine
// (`super::generic` / `super::generic_object`), which already models an Array
// subclass correctly — it is the same engine the UNANNOTATED path has always
// used via `js_native_call_method`.
//
// Unlike #7573's Map/Set fix there is nothing to *redirect* to: an Array
// subclass instance has no hidden backing collection (`js_array_subclass_init`
// installs a `length` own property and the elements are ordinary indexed object
// properties — see `node_stream_constructors/builders.rs`). Minting one would
// split element storage in two, since `Object.keys` / `for…in` /
// `JSON.stringify` / the generic engine all read the object's own properties;
// the answer here is therefore "run the generic engine", not "redirect".
// ---------------------------------------------------------------------------

/// The array-like OBJECT receiver a raw `js_array_*` entry point must actually
/// run on, or `None` when the pointer is not one.
///
/// Admits exactly what [`super::generic::plain_object_value`] admits — an
/// object literal, an anonymous shape, or a `class X extends Array` instance —
/// so ordinary user-class instances, real arrays, typed arrays, buffers, and
/// proxies all answer `None` and keep their existing behaviour.
///
/// Marked `#[cold]`/`#[inline(never)]`: every caller reaches it only from a
/// branch `clean_arr_ptr` already refused, so a genuine `ArrayHeader` never
/// executes a byte of this.
/// One-load brand pre-filter: true only when `arr` has a readable `GcHeader`
/// saying `GC_TYPE_OBJECT`. A genuine `ArrayHeader` answers false without
/// touching a side table, so callers can gate the (registry-probing)
/// [`array_object_receiver`] behind it on a hot path. Uses
/// `addr_class::try_read_gc_header`, which magnitude-classifies the address
/// before any dereference.
#[inline]
pub(crate) fn raw_receiver_is_heap_object(arr: *const ArrayHeader) -> bool {
    let raw = ((arr as u64) & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw == 0 {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(raw) } {
        Some(header) => header.obj_type == crate::gc::GC_TYPE_OBJECT,
        None => false,
    }
}

#[cold]
#[inline(never)]
pub(crate) fn array_object_receiver(arr: *const ArrayHeader) -> Option<f64> {
    let raw = (arr as u64) & 0x0000_FFFF_FFFF_FFFF;
    if raw == 0 {
        return None;
    }
    super::generic::plain_object_value(raw as *const ArrayHeader)
}

/// True when `value` is a live `class X extends Array` INSTANCE — the
/// annotation-independent brand test the generic `[[Set]]` funnels use to
/// decide whether the Array-exotic `length` steps apply.
pub(crate) fn is_array_subclass_value(value: f64) -> bool {
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return false;
    }
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    // `raw_receiver_is_heap_object` magnitude-classifies through
    // `addr_class::try_read_gc_header` and proves `GC_TYPE_OBJECT` before the
    // class-id read below dereferences the header.
    if !raw_receiver_is_heap_object(raw as *const ArrayHeader) {
        return false;
    }
    let class_id = crate::object::js_object_get_class_id(raw);
    class_id != 0 && is_array_subclass_class_id(class_id)
}

/// Run an `Array.prototype` method generically on the array-like object
/// `recv`, covering both the mutating family (`push` / `pop` / `shift` /
/// `unshift` / `reverse` / `splice` / `sort` / `concat`) and the read family
/// (`map` / `filter` / `forEach` / `join` / `slice` / `indexOf` / …).
///
/// Returns `None` only for a method name neither engine implements.
#[cold]
#[inline(never)]
pub(crate) fn array_object_method(recv: f64, method: &str, args: &[f64]) -> Option<f64> {
    let (ptr, len) = (args.as_ptr(), args.len());
    if let Some(result) = super::generic::run_object_mutator(recv, method, ptr, len) {
        return Some(result);
    }
    super::generic::dispatch_arraylike_read_method(recv, method, ptr, len)
}

/// `Get(recv, ToString(index))` for an array-like object receiver.
#[cold]
#[inline(never)]
pub(crate) fn array_object_index_get(recv: f64, index: u32) -> f64 {
    al_get(recv, index as i64)
}

/// `Set(recv, ToString(index), value, …)` PLUS the Array-exotic `length`
/// maintenance the receiver's class inherits from `Array`.
///
/// A `class X extends Array` instance is a real Array in JavaScript, so
/// `sub[3] = v` sets `length` to 4 (ECMA-262 §10.4.2.1
/// `ArraySetLength`/`ArrayDefineOwnProperty`). Perry models the instance as a
/// plain object, whose `[[DefineOwnProperty]]` has no such step — pre-fix
/// `sub[0] = 10; sub.length` read back `0`, on the ANNOTATED and unannotated
/// paths alike. Emulate the exotic step here so both agree with node.
#[cold]
#[inline(never)]
pub(crate) fn array_object_index_set(recv: f64, index: u32, value: f64) {
    // The store interns a key string and can allocate, so root the receiver
    // across it — it is a movable `ObjectHeader` and is read again below.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    crate::object::js_object_set_index_polymorphic(
        (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
        index as f64,
        value,
    );
    maintain_array_exotic_length(handle.get_nanbox_f64(), index);
}

/// The Array-exotic post-step for an indexed own-property write, applied by
/// the two generic OBJECT store funnels (`js_put_value_set` and
/// `js_object_set_index_polymorphic`) AFTER the store has landed.
///
/// `sub[3] = v` on a `class X extends Array` instance must leave `length == 4`.
/// Perry models the instance as a plain object, so nothing in its
/// `[[DefineOwnProperty]]` does that — pre-fix `sub[0] = 10; sub.length` read
/// back `0` on the annotated AND unannotated paths alike, which then made the
/// next `sub.push(v)` append at index 0 and overwrite the element.
///
/// Gated on the receiver's class chain reaching `Array`, so an object literal
/// (`class_id == 0`) short-circuits on one load and an ordinary class instance
/// on a bounded parent walk. `key` is a property-key VALUE; a non-canonical
/// array index (`"length"`, `"foo"`, `"01"`, a symbol) is a no-op.
pub(crate) fn note_array_subclass_index_write(recv: f64, key: f64) {
    // Stringifying a numeric key can allocate and evacuate the object. Keep
    // both inputs live, then re-read the receiver before retiring its proof.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let key_h = scope.root_nanbox_f64(key);
    if !is_array_subclass_value(recv_h.get_nanbox_f64()) {
        return;
    }
    let key_ptr = crate::value::js_jsvalue_to_string(key_h.get_nanbox_f64())
        as *const crate::string::StringHeader;
    // The `&str` borrows the heap `StringHeader`'s bytes. `canonical_array_index`
    // only parses digits — it allocates nothing, so the borrow cannot straddle a
    // collection point (the `&[u8]`-into-a-StringHeader hazard in CLAUDE.md).
    let index = unsafe {
        match crate::object::has_own_helpers::str_from_string_header(key_ptr)
            .and_then(crate::object::canonical_array_index)
        {
            Some(i) => i,
            None => return,
        }
    };
    let live_recv = recv_h.get_nanbox_f64();
    let raw = (live_recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    // A successful value overwrite does not necessarily change shape and a
    // pointer-free tag (notably an SSO string) needs no GC layout note. Retire
    // the numeric authority explicitly so neither case can reuse stale proof.
    unsafe { clear_packed_subclass_numeric_proof(raw) };
    maintain_array_exotic_length(live_recv, index);
}

/// The `length`-bumping half of `array_object_index_set`, split out so the
/// generic OBJECT index-store funnels can apply it without re-entering the
/// store.
pub(crate) fn maintain_array_exotic_length(recv: f64, index: u32) {
    let current = al_length(recv);
    if (index as i64) < current {
        return;
    }
    // `js_string_from_bytes` ALLOCATES, so it is a collection point: root the
    // receiver and re-read it afterwards rather than deriving the raw pointer
    // first (the #7192 store-after-an-allocating-call shape — a movable
    // `ObjectHeader` written through a pre-allocation address lands on a
    // forwarding stub).
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    crate::object::js_object_set_field_by_name(raw, key, (index as f64) + 1.0);
}

/// `Set(recv, "length", new_length, true)` for an array-like object receiver:
/// truncating deletes the indices at or above the new length, exactly as the
/// Array-exotic `[[DefineOwnProperty]]` would.
#[cold]
#[inline(never)]
pub(crate) fn array_object_set_length(recv: f64, new_length: f64) {
    if !new_length.is_finite() || new_length < 0.0 || new_length.trunc() != new_length {
        crate::array::array_length_range_error();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    let target = new_length as i64;
    let current = al_length(handle.get_nanbox_f64());
    for k in target..current {
        let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
        crate::object::js_object_delete_dynamic(raw, k as f64);
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    crate::object::set_field_by_name_object_tail(raw, key, new_length);
}
