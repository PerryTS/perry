//! Exact-size final output for bounded plain records and primitive-array fields.
//! The plan contains only byte counts, array indices and inline scalar text.
//! Only the parent object is rooted; child pointers are rederived after allocation.

use super::stringify_flat::{
    bounded_keys_are_dense, emit_piece, key_piece, scalar_piece, slot, Piece,
};
use super::*;
use crate::string::{init_string_header, string_storage_alloc};
use std::cell::UnsafeCell;

const MAX_FIELDS: usize = 8;
const MAX_ELEMENTS: usize = 16;
const KEY_PREFIX_CACHE_SLOTS: usize = 8;
const MAX_KEY_PREFIX_BYTES: usize = 256;
const OBJECT_BYTES: usize = std::mem::size_of::<crate::ObjectHeader>();
const ARRAY_BYTES: usize = std::mem::size_of::<crate::ArrayHeader>();

#[derive(Clone, Copy)]
struct KeyPrefixPlan {
    shape: u32,
    candidate: u32,
    fields: u8,
    bytes: u16,
    units: u16,
    offsets: [u16; MAX_FIELDS + 1],
    data: [u8; MAX_KEY_PREFIX_BYTES],
}

const EMPTY_KEY_PREFIX_PLAN: KeyPrefixPlan = KeyPrefixPlan {
    shape: 0,
    candidate: 0,
    fields: 0,
    bytes: 0,
    units: 0,
    offsets: [0; MAX_FIELDS + 1],
    data: [0; MAX_KEY_PREFIX_BYTES],
};

crate::perry_thread_local! {
    /// Stable shape IDs index copied native bytes only. No managed pointer or
    /// value survives a stringify call, so the collector has no cache edge to
    /// trace or rewrite.
    static KEY_PREFIX_CACHE: UnsafeCell<[KeyPrefixPlan; KEY_PREFIX_CACHE_SLOTS]> =
        const { UnsafeCell::new([EMPTY_KEY_PREFIX_PLAN; KEY_PREFIX_CACHE_SLOTS]) };
}

#[derive(Clone, Copy)]
enum Field {
    Scalar(Piece),
    Array { start: usize, len: usize },
}

/// Decline before entering the planning frame on large/exotic receivers.
#[inline]
pub(super) unsafe fn try_object(bits: u64) -> Option<JSValue> {
    if bits & crate::value::TAG_MASK != POINTER_TAG {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *const crate::ObjectHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(obj as usize)?.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + OBJECT_BYTES
        || (*obj).class_id != 0
    {
        return None;
    }
    let keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        return None;
    }
    let fields = (*keys).length as usize;
    if fields == 0
        || fields > MAX_FIELDS
        || fields > (*keys).capacity as usize
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + OBJECT_BYTES + fields * 8
        || fields
            > crate::object::object_live_slot_count(obj)
                .max(crate::object::INLINE_SLOT_FLOOR as u32) as usize
        || !bounded_keys_are_dense(keys, fields)
    {
        return None;
    }
    emit_record(obj, fields)
}

unsafe fn dense_array(bits: u64) -> Option<(*const crate::ArrayHeader, usize)> {
    if bits & crate::value::TAG_MASK != POINTER_TAG {
        return None;
    }
    let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(arr as usize)?.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + ARRAY_BYTES
    {
        return None;
    }
    let len = (*arr).length as usize;
    if len > MAX_ELEMENTS
        || len > (*arr).capacity as usize
        || (header.size as usize) < crate::gc::GC_HEADER_SIZE + ARRAY_BYTES + len * 8
        || crate::object::prototype_chain::object_static_prototype(arr as usize).is_some()
    {
        return None;
    }
    Some((arr, len))
}

/// Return a stable native prefix plan for this exact immutable shape. The
/// caller may retain the pointer across final-output allocation: entries are
/// thread-local, contain no managed pointers, and this callback-free path
/// cannot recursively replace the cache slot.
unsafe fn key_prefix_plan(
    obj: *const crate::ObjectHeader,
    keys: *const crate::ArrayHeader,
    fields: usize,
) -> Option<*const KeyPrefixPlan> {
    let shape = crate::object::shapes::object_shape_stamp(obj);
    if shape == 0 {
        return None;
    }
    KEY_PREFIX_CACHE.with(|cache| {
        let plans = &mut *cache.get();
        let cache_slot = (shape.wrapping_mul(0x9e37_79b9) as usize) & (KEY_PREFIX_CACHE_SLOTS - 1);
        let cached = &mut plans[cache_slot];
        if cached.shape == shape {
            if cached.fields as usize == fields {
                return Some(cached as *const KeyPrefixPlan);
            }
            if cached.fields == 0 {
                // A zero-field record never reaches this path, so this marks a
                // shape whose keys cannot fit or cannot use cached emission.
                return None;
            }
        }
        // A one-off shape should pay only the existing record path. Build a
        // native plan after the same shape reaches this slot twice; consecutive
        // distinct misses therefore avoid copying keys into cold plans.
        if cached.candidate != shape {
            cached.candidate = shape;
            return None;
        }

        let built = (|| {
            let mut plan = EMPTY_KEY_PREFIX_PLAN;
            plan.shape = shape;
            plan.fields = fields as u8;
            let mut at = 0usize;
            let mut units = 0u32;
            for i in 0..fields {
                plan.offsets[i] = at as u16;
                plan.data[at] = if i == 0 { b'{' } else { b',' };
                at += 1;
                units += 1;
                let key_bits = slot(keys.cast(), ARRAY_BYTES, i);
                let key = key_piece(key_bits)?;
                let (key_bytes, key_units) = key.lengths();
                let needed = at.checked_add(key_bytes as usize)?.checked_add(1)?;
                if needed > MAX_KEY_PREFIX_BYTES {
                    return None;
                }
                at += emit_piece(key, key_bits, plan.data.as_mut_ptr().add(at));
                plan.data[at] = b':';
                at += 1;
                units = units.checked_add(key_units)?.checked_add(1)?;
                plan.offsets[i + 1] = at as u16;
            }
            plan.bytes = at as u16;
            plan.units = u16::try_from(units).ok()?;
            Some(plan)
        })();
        let Some(plan) = built else {
            cached.shape = shape;
            cached.candidate = 0;
            cached.fields = 0;
            return None;
        };
        *cached = plan;
        Some(cached as *const KeyPrefixPlan)
    })
}

#[inline(never)]
unsafe fn emit_cached_record(
    obj: *const crate::ObjectHeader,
    fields: usize,
    prefix: *const KeyPrefixPlan,
) -> Option<JSValue> {
    let mut value_plan: [std::mem::MaybeUninit<Field>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    let mut elements: [std::mem::MaybeUninit<Piece>; MAX_ELEMENTS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_ELEMENTS];
    let mut used = 0;
    let (mut bytes, mut units) = (1u32 + (*prefix).bytes as u32, 1u32 + (*prefix).units as u32);
    for i in 0..fields {
        let bits = slot(obj.cast(), OBJECT_BYTES, i);
        let (vb, vu) = if let Some(value) = scalar_piece(bits) {
            value_plan[i].write(Field::Scalar(value));
            value.lengths()
        } else {
            let (arr, len) = dense_array(bits)?;
            if used + len > MAX_ELEMENTS {
                return None;
            }
            value_plan[i].write(Field::Array { start: used, len });
            let (mut ab, mut au) = (2u32, 2u32);
            for j in 0..len {
                let value = scalar_piece(slot(arr.cast(), ARRAY_BYTES, j))?;
                elements[used + j].write(value);
                let (eb, eu) = value.lengths();
                let comma = u32::from(j != 0);
                ab = ab.checked_add(eb)?.checked_add(comma)?;
                au = au.checked_add(eu)?.checked_add(comma)?;
            }
            used += len;
            (ab, au)
        };
        bytes = bytes.checked_add(vb)?;
        units = units.checked_add(vu)?;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    if !super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast()) {
        return None;
    }
    let (result, output) = string_storage_alloc(bytes);
    init_string_header(result, units, bytes, bytes, 0, 0);
    input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        let mut at = 0usize;
        for i in 0..fields {
            let start = (*prefix).offsets[i] as usize;
            let end = (*prefix).offsets[i + 1] as usize;
            super::stringify_copy::copy_bytes(
                (*prefix).data.as_ptr().add(start),
                output.add(at),
                end - start,
            );
            at += end - start;
            let bits = slot(obj.cast(), OBJECT_BYTES, i);
            match value_plan[i].assume_init() {
                Field::Scalar(value) => at += emit_piece(value, bits, output.add(at)),
                Field::Array { start, len } => {
                    let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
                    // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                    output.add(at).write(b'[');
                    at += 1;
                    for j in 0..len {
                        if j != 0 {
                            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                            output.add(at).write(b',');
                            at += 1;
                        }
                        at += emit_piece(
                            elements[start + j].assume_init(),
                            slot(arr.cast(), ARRAY_BYTES, j),
                            output.add(at),
                        );
                    }
                    // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                    output.add(at).write(b']');
                    at += 1;
                }
            }
        }
        // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
        output.add(at).write(b'}');
        debug_assert_eq!(at + 1, bytes as usize);
        Some(JSValue::string_ptr(result))
    })
}

#[inline(never)]
unsafe fn emit_record(obj: *const crate::ObjectHeader, fields: usize) -> Option<JSValue> {
    let keys = crate::object::object_keys_array(obj);
    if let Some(prefix) = key_prefix_plan(obj, keys, fields) {
        return emit_cached_record(obj, fields, prefix);
    }
    // Only the validated prefixes are read during emission. Avoid clearing
    // the full fixed-capacity scratch frame for short records; every accessed
    // slot below is written before the output allocation can occur.
    let mut key_plan: [std::mem::MaybeUninit<Piece>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    let mut value_plan: [std::mem::MaybeUninit<Field>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    let mut elements: [std::mem::MaybeUninit<Piece>; MAX_ELEMENTS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_ELEMENTS];
    let mut used = 0;
    let (mut bytes, mut units) = (2u32, 2u32);
    for i in 0..fields {
        let key = key_piece(slot(keys.cast(), ARRAY_BYTES, i))?;
        key_plan[i].write(key);
        let (kb, ku) = key.lengths();
        let bits = slot(obj.cast(), OBJECT_BYTES, i);
        let (vb, vu) = if let Some(value) = scalar_piece(bits) {
            value_plan[i].write(Field::Scalar(value));
            value.lengths()
        } else {
            let (arr, len) = dense_array(bits)?;
            if used + len > MAX_ELEMENTS {
                return None;
            }
            value_plan[i].write(Field::Array { start: used, len });
            let (mut ab, mut au) = (2u32, 2u32);
            for j in 0..len {
                let value = scalar_piece(slot(arr.cast(), ARRAY_BYTES, j))?;
                elements[used + j].write(value);
                let (eb, eu) = value.lengths();
                let comma = u32::from(j != 0);
                ab = ab.checked_add(eb)?.checked_add(comma)?;
                au = au.checked_add(eu)?.checked_add(comma)?;
            }
            used += len;
            (ab, au)
        };
        let punctuation = 1 + u32::from(i != 0);
        bytes = bytes
            .checked_add(kb)?
            .checked_add(vb)?
            .checked_add(punctuation)?;
        units = units
            .checked_add(ku)?
            .checked_add(vu)?
            .checked_add(punctuation)?;
    }
    // Root the complete input graph before either prototype initialization or
    // final output allocation. Nothing in the stack plan is a heap pointer.
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    if !super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast()) {
        return None;
    }
    let (result, output) = string_storage_alloc(bytes);
    init_string_header(result, units, bytes, bytes, 0, 0);
    input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        let keys = crate::object::object_keys_array(obj);
        output.write(b'{');
        let mut at = 1usize;
        for i in 0..fields {
            if i != 0 {
                // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                output.add(at).write(b',');
                at += 1;
            }
            at += emit_piece(
                key_plan[i].assume_init(),
                slot(keys.cast(), ARRAY_BYTES, i),
                output.add(at),
            );
            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
            output.add(at).write(b':');
            at += 1;
            let bits = slot(obj.cast(), OBJECT_BYTES, i);
            match value_plan[i].assume_init() {
                Field::Scalar(value) => at += emit_piece(value, bits, output.add(at)),
                Field::Array { start, len } => {
                    let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
                    // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                    output.add(at).write(b'[');
                    at += 1;
                    for j in 0..len {
                        if j != 0 {
                            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                            output.add(at).write(b',');
                            at += 1;
                        }
                        at += emit_piece(
                            elements[start + j].assume_init(),
                            slot(arr.cast(), ARRAY_BYTES, j),
                            output.add(at),
                        );
                    }
                    // GC_STORE_AUDIT(POINTER_FREE): JSON output payload byte.
                    output.add(at).write(b']');
                    at += 1;
                }
            }
        }
        // GC_STORE_AUDIT(POINTER_FREE): JSON output payload byte.
        output.add(at).write(b'}');
        debug_assert_eq!(at + 1, bytes as usize);
        Some(JSValue::string_ptr(result))
    })
}

#[cfg(test)]
#[path = "stringify_record_output_tests.rs"]
mod tests;
