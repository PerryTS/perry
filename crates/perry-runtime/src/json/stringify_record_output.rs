//! Exact-size final output for bounded plain records and primitive-array fields.
//! The plan contains only byte counts, array indices and inline scalar text.
//! Only the parent object is rooted; child pointers are rederived after allocation.

use super::stringify_flat::{
    bounded_keys_are_dense, emit_piece, key_piece, scalar_piece, slot, Piece,
};
use super::*;
use crate::string::{
    init_string_header, json_output_storage_alloc, string_storage_alloc,
    JSON_MALLOC_OUTPUT_THRESHOLD,
};
use std::cell::UnsafeCell;

const MAX_FIELDS: usize = 8;
const MAX_ELEMENTS: usize = 16;
const KEY_PREFIX_CACHE_SLOTS: usize = 8;
const MAX_KEY_PREFIX_BYTES: usize = 256;
const MAX_REPEATED_OUTPUT_BYTES: usize = 256;
const SCALAR_FIELD: u8 = u8::MAX;
const OBJECT_BYTES: usize = std::mem::size_of::<crate::ObjectHeader>();
const ARRAY_BYTES: usize = std::mem::size_of::<crate::ArrayHeader>();

#[derive(Clone, Copy)]
struct KeyPrefixPlan {
    shape: u32,
    candidate: u32,
    /// Address token for the last receiver that passed the complete prototype
    /// proof. Never dereferenced; movement turns it into a cache miss.
    receiver: usize,
    receiver_epoch: u64,
    fields: u8,
    memo_cooldown: u8,
    bytes: u16,
    units: u16,
    offsets: [u16; MAX_FIELDS + 1],
    data: [u8; MAX_KEY_PREFIX_BYTES],
}

const EMPTY_KEY_PREFIX_PLAN: KeyPrefixPlan = KeyPrefixPlan {
    shape: 0,
    candidate: 0,
    receiver: 0,
    receiver_epoch: 0,
    fields: 0,
    memo_cooldown: 0,
    bytes: 0,
    units: 0,
    offsets: [0; MAX_FIELDS + 1],
    data: [0; MAX_KEY_PREFIX_BYTES],
};

#[derive(Clone, Copy)]
struct RepeatedOutput {
    /// Identity token only. Never dereferenced; a moving collection turns a
    /// pre-allocation token into a miss on the next call.
    receiver: usize,
    candidate_receiver: usize,
    epoch: u64,
    candidate_signature: u64,
    candidate_misses: u8,
    shape: u32,
    fields: u8,
    bytes: u16,
    units: u16,
    array_lengths: [u8; MAX_FIELDS],
    field_bits: [u64; MAX_FIELDS],
    element_bits: [u64; MAX_ELEMENTS],
    data: [u8; MAX_REPEATED_OUTPUT_BYTES],
}

const EMPTY_REPEATED_OUTPUT: RepeatedOutput = RepeatedOutput {
    receiver: 0,
    candidate_receiver: 0,
    epoch: 0,
    candidate_signature: 0,
    candidate_misses: 0,
    shape: 0,
    fields: 0,
    bytes: 0,
    units: 0,
    array_lengths: [SCALAR_FIELD; MAX_FIELDS],
    field_bits: [0; MAX_FIELDS],
    element_bits: [0; MAX_ELEMENTS],
    data: [0; MAX_REPEATED_OUTPUT_BYTES],
};

const _: () = assert!(std::mem::size_of::<RepeatedOutput>() <= 512);

crate::perry_thread_local! {
    /// Stable shape IDs index copied native bytes only. No managed pointer or
    /// value survives a stringify call, so the collector has no cache edge to
    /// trace or rewrite.
    static KEY_PREFIX_CACHE: UnsafeCell<[KeyPrefixPlan; KEY_PREFIX_CACHE_SLOTS]> =
        const { UnsafeCell::new([EMPTY_KEY_PREFIX_PLAN; KEY_PREFIX_CACHE_SLOTS]) };

    /// A bounded native copy of the last small record result and exact value
    /// tokens. It retains no managed allocation and needs no GC scanning.
    static REPEATED_OUTPUT: UnsafeCell<RepeatedOutput> =
        const { UnsafeCell::new(EMPTY_REPEATED_OUTPUT) };
}

#[cfg(test)]
crate::perry_thread_local! {
    static RECEIVER_PROOF_MISSES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static REPEATED_OUTPUT_HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
enum Field {
    Scalar(Piece),
    Array { start: usize, len: usize },
}

/// Plan a cached record value, reusing the parser's proof for plain heap
/// strings. `string_from_json_bytes` sets this flag only for tokens without a
/// backslash; JSON syntax already excludes unescaped quotes, controls and
/// incomplete UTF-8. Newly constructed or mutated strings keep the ordinary
/// scalar planner and its full scan. Keeping this specialization in the
/// record-output module also leaves tiny-object dispatch byte-identical.
#[inline]
unsafe fn record_value_piece(bits: u64) -> Option<Piece> {
    if bits & crate::value::TAG_MASK == STRING_TAG {
        let header = (bits & POINTER_MASK) as *const StringHeader;
        if !header.is_null() && (*header).flags & crate::string::STRING_FLAG_JSON_ESCAPE_FREE != 0 {
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let (source, len) =
                crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)?;
            if source.is_null() || len > u32::MAX - 2 {
                return None;
            }
            let units = (*header).utf16_len;
            units.checked_add(2)?;
            return Some(Piece::String { bytes: len, units });
        }
    }
    scalar_piece(bits)
}

/// Reuse the complete `toJSON` miss for repeated serialization of the same
/// receiver. The address is only an identity token: moving GC changes it, and
/// address reuse cannot revive a verdict after an explicit-prototype or
/// property-semantic change because both advance the shared semantic epoch.
/// Object.prototype `toJSON` writes advance that epoch at the mutation site,
/// keeping this hit check to one address and one integer comparison. The shape
/// plan has already proved that no own key can expose `toJSON`.
#[inline]
unsafe fn receiver_to_json_absent(
    obj: *const crate::ObjectHeader,
    plan: *mut KeyPrefixPlan,
) -> bool {
    let epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
    if (*plan).receiver == obj as usize && (*plan).receiver_epoch == epoch {
        return true;
    }
    #[cfg(test)]
    RECEIVER_PROOF_MISSES.with(|count| count.set(count.get() + 1));
    if !super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast()) {
        return false;
    }
    (*plan).receiver = obj as usize;
    (*plan).receiver_epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
    true
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
    if let Some(result) = emit_repeated_output(obj, header) {
        return Some(result);
    }
    let keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        return emit_empty_object(obj);
    }
    let fields = (*keys).length as usize;
    if fields == 0 {
        return emit_empty_object(obj);
    }
    if fields > MAX_FIELDS
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

#[inline]
unsafe fn emit_empty_object(obj: *const crate::ObjectHeader) -> Option<JSValue> {
    if !super::stringify_tojson_probe::to_json_definitely_absent_without_gc(obj.cast()) {
        return None;
    }
    REPEATED_OUTPUT.with(|entry| {
        let cached = &mut *entry.get();
        cached.receiver = 0;
        cached.epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
        cached.shape = crate::object::shapes::object_shape_stamp(obj);
        cached.fields = 0;
        cached.bytes = 2;
        cached.units = 2;
        cached.data[..2].copy_from_slice(b"{}");
        cached.receiver = obj as usize;
    });
    Some(JSValue::short_string_unchecked(b"{}"))
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
        || (crate::object::prototype_chain::array_static_proto_recorded()
            && crate::object::prototype_chain::object_static_prototype(arr as usize).is_some())
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
) -> Option<*mut KeyPrefixPlan> {
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
                return Some(cached as *mut KeyPrefixPlan);
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
                let key_bits = slot(crate::array::array_elements_ptr(keys).cast(), 0, i);
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
        Some(cached as *mut KeyPrefixPlan)
    })
}

/// Copy a previously completed small record after proving that every
/// observable input bit is unchanged. The verification happens before the
/// output allocation. No managed pointer is needed afterward, so a collection
/// during allocation may move the input without adding a temporary root.
#[inline(never)]
unsafe fn emit_repeated_output(
    obj: *const crate::ObjectHeader,
    header: &crate::gc::GcHeader,
) -> Option<JSValue> {
    REPEATED_OUTPUT.with(|entry| {
        let cached = &mut *entry.get();
        if cached.receiver != obj as usize {
            return None;
        }
        let fields = cached.fields as usize;
        if cached.epoch != crate::object::prop_plan::prop_plan_semantic_epoch()
            || fields > MAX_FIELDS
            || cached.bytes == 0
            || (header.size as usize) < crate::gc::GC_HEADER_SIZE + OBJECT_BYTES + fields * 8
            || fields
                > crate::object::object_live_slot_count(obj)
                    .max(crate::object::INLINE_SLOT_FLOOR as u32) as usize
            || crate::object::shapes::object_shape_stamp(obj) != cached.shape
        {
            cached.receiver = 0;
            return None;
        }
        if fields == 0 {
            if cached.bytes != 2 || cached.data[..2] != *b"{}" {
                cached.receiver = 0;
                return None;
            }
            #[cfg(test)]
            REPEATED_OUTPUT_HITS.with(|hits| hits.set(hits.get() + 1));
            return Some(JSValue::short_string_unchecked(b"{}"));
        }
        let mut element = 0usize;
        for i in 0..fields {
            let bits = slot(obj.cast(), OBJECT_BYTES, i);
            if bits != cached.field_bits[i] {
                cached.receiver = 0;
                return None;
            }
            let len = cached.array_lengths[i];
            if len == SCALAR_FIELD {
                continue;
            }
            let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
            let header =
                &*((arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
            if header.obj_type != crate::gc::GC_TYPE_ARRAY
                || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
                || (*arr).length != len as u32
                || (*arr).capacity < len as u32
                || (header.size as usize)
                    < crate::gc::GC_HEADER_SIZE + ARRAY_BYTES + len as usize * 8
                || element + len as usize > MAX_ELEMENTS
            {
                cached.receiver = 0;
                return None;
            }
            for j in 0..len as usize {
                if slot(crate::array::array_elements_ptr(arr).cast(), 0, j)
                    != cached.element_bits[element + j]
                {
                    cached.receiver = 0;
                    return None;
                }
            }
            element += len as usize;
        }

        #[cfg(test)]
        REPEATED_OUTPUT_HITS.with(|hits| hits.set(hits.get() + 1));
        let bytes = cached.bytes as u32;
        let units = cached.units as u32;
        let (result, output) = string_storage_alloc(bytes);
        init_string_header(result, units, bytes, bytes, 0, 0);
        super::stringify_copy::copy_bytes(cached.data.as_ptr(), output, bytes as usize);
        Some(JSValue::string_ptr(result))
    })
}

#[inline]
fn repeated_signature_mix(signature: u64, bits: u64) -> u64 {
    signature.rotate_left(13) ^ bits.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

#[inline(never)]
unsafe fn emit_cached_record_uncached(
    obj: *const crate::ObjectHeader,
    fields: usize,
    prefix: *mut KeyPrefixPlan,
) -> Option<JSValue> {
    let mut value_plan: [std::mem::MaybeUninit<Field>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    let mut elements: [std::mem::MaybeUninit<Piece>; MAX_ELEMENTS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_ELEMENTS];
    let mut used = 0;
    let (mut bytes, mut units) = (1u32 + (*prefix).bytes as u32, 1u32 + (*prefix).units as u32);
    for i in 0..fields {
        let bits = slot(obj.cast(), OBJECT_BYTES, i);
        let (vb, vu) = if let Some(value) = record_value_piece(bits) {
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
                let value =
                    record_value_piece(slot(crate::array::array_elements_ptr(arr).cast(), 0, j))?;
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

    let large_output = bytes >= JSON_MALLOC_OUTPUT_THRESHOLD;
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    if large_output {
        super::stringify_flat::service_json_output_sweep_boundary();
    }
    if !receiver_to_json_absent(obj, prefix) {
        return None;
    }
    let construction = large_output.then(crate::gc::GcSuppressScope::new);
    let (result, output) = json_output_storage_alloc(bytes);
    init_string_header(result, units, bytes, bytes, 0, 0);
    let value = input.with_const_ptr(|obj: *const crate::ObjectHeader| {
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
                            slot(crate::array::array_elements_ptr(arr).cast(), 0, j),
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
    });
    drop(construction);
    if large_output {
        super::stringify_flat::note_completed_malloc_json_output(bytes);
    }
    value
}

#[inline(never)]
unsafe fn emit_cached_record_memo(
    obj: *const crate::ObjectHeader,
    fields: usize,
    prefix: *mut KeyPrefixPlan,
) -> Option<JSValue> {
    let mut value_plan: [std::mem::MaybeUninit<Field>; MAX_FIELDS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_FIELDS];
    let mut elements: [std::mem::MaybeUninit<Piece>; MAX_ELEMENTS] =
        [const { std::mem::MaybeUninit::uninit() }; MAX_ELEMENTS];
    let mut used = 0;
    let (mut bytes, mut units) = (1u32 + (*prefix).bytes as u32, 1u32 + (*prefix).units as u32);
    for i in 0..fields {
        let bits = slot(obj.cast(), OBJECT_BYTES, i);
        let (vb, vu) = if let Some(value) = record_value_piece(bits) {
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
                let value =
                    record_value_piece(slot(crate::array::array_elements_ptr(arr).cast(), 0, j))?;
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

    let large_output = bytes >= JSON_MALLOC_OUTPUT_THRESHOLD;
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    if large_output {
        super::stringify_flat::service_json_output_sweep_boundary();
    }
    if !receiver_to_json_absent(obj, prefix) {
        return None;
    }
    let repeated_candidate = bytes as usize <= MAX_REPEATED_OUTPUT_BYTES;
    let construction = large_output.then(crate::gc::GcSuppressScope::new);
    let (result, output) = json_output_storage_alloc(bytes);
    init_string_header(result, units, bytes, bytes, 0, 0);
    let value = input.with_const_ptr(|obj: *const crate::ObjectHeader| {
        let mut at = 0usize;
        let mut signature = (*prefix).shape as u64 ^ fields as u64;
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
            if repeated_candidate {
                signature = repeated_signature_mix(signature, bits);
            }
            match value_plan[i].assume_init() {
                Field::Scalar(value) => at += emit_piece(value, bits, output.add(at)),
                Field::Array { start, len } => {
                    let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
                    if repeated_candidate {
                        signature = repeated_signature_mix(signature, len as u64);
                    }
                    // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                    output.add(at).write(b'[');
                    at += 1;
                    for j in 0..len {
                        if j != 0 {
                            // GC_STORE_AUDIT(POINTER_FREE): JSON byte-buffer payload.
                            output.add(at).write(b',');
                            at += 1;
                        }
                        let element_bits = slot(crate::array::array_elements_ptr(arr).cast(), 0, j);
                        if repeated_candidate {
                            signature = repeated_signature_mix(signature, element_bits);
                        }
                        at += emit_piece(
                            elements[start + j].assume_init(),
                            element_bits,
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
        if repeated_candidate {
            REPEATED_OUTPUT.with(|entry| {
                let cached = &mut *entry.get();
                let install = cached.candidate_receiver == obj as usize
                    && cached.candidate_signature == signature;
                let mismatch = cached.candidate_receiver != 0 && !install;
                cached.candidate_receiver = obj as usize;
                cached.candidate_signature = signature;
                if !install {
                    if mismatch {
                        cached.candidate_misses = cached.candidate_misses.saturating_add(1);
                        if cached.candidate_misses >= 2 {
                            cached.candidate_misses = 0;
                            cached.candidate_receiver = 0;
                            (*prefix).memo_cooldown = 64;
                        }
                    }
                    return;
                }

                // Publish the receiver last. Until then an allocation or a
                // partially copied signature cannot produce a cache hit.
                cached.receiver = 0;
                cached.array_lengths.fill(SCALAR_FIELD);
                for i in 0..fields {
                    let bits = slot(obj.cast(), OBJECT_BYTES, i);
                    cached.field_bits[i] = bits;
                    if let Field::Array { start, len } = value_plan[i].assume_init() {
                        cached.array_lengths[i] = len as u8;
                        let arr = (bits & POINTER_MASK) as *const crate::ArrayHeader;
                        for j in 0..len {
                            cached.element_bits[start + j] =
                                slot(crate::array::array_elements_ptr(arr).cast(), 0, j);
                        }
                    }
                }
                super::stringify_copy::copy_bytes(output, cached.data.as_mut_ptr(), bytes as usize);
                cached.epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
                cached.shape = (*prefix).shape;
                cached.fields = fields as u8;
                cached.bytes = bytes as u16;
                cached.units = units as u16;
                cached.candidate_misses = 0;
                (*prefix).memo_cooldown = 0;
                cached.receiver = obj as usize;
            });
        }
        Some(JSValue::string_ptr(result))
    });
    drop(construction);
    if large_output {
        super::stringify_flat::note_completed_malloc_json_output(bytes);
    }
    value
}

#[inline(never)]
unsafe fn emit_record(obj: *const crate::ObjectHeader, fields: usize) -> Option<JSValue> {
    let keys = crate::object::object_keys_array(obj);
    if let Some(prefix) = key_prefix_plan(obj, keys, fields) {
        let repeat_eligible = (*prefix).receiver == obj as usize
            && (*prefix).receiver_epoch == crate::object::prop_plan::prop_plan_semantic_epoch();
        if repeat_eligible {
            if (*prefix).memo_cooldown != 0 {
                (*prefix).memo_cooldown -= 1;
                return emit_cached_record_uncached(obj, fields, prefix);
            }
            return emit_cached_record_memo(obj, fields, prefix);
        }
        return emit_cached_record_uncached(obj, fields, prefix);
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
        let key = key_piece(slot(crate::array::array_elements_ptr(keys).cast(), 0, i))?;
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
                let value = scalar_piece(slot(crate::array::array_elements_ptr(arr).cast(), 0, j))?;
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
    let large_output = bytes >= JSON_MALLOC_OUTPUT_THRESHOLD;
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(obj);
    if large_output {
        super::stringify_flat::service_json_output_sweep_boundary();
    }
    if !super::stringify_tojson_probe::to_json_definitely_absent_after_own_keys(obj.cast()) {
        return None;
    }
    let construction = large_output.then(crate::gc::GcSuppressScope::new);
    let (result, output) = json_output_storage_alloc(bytes);
    init_string_header(result, units, bytes, bytes, 0, 0);
    let value = input.with_const_ptr(|obj: *const crate::ObjectHeader| {
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
                slot(crate::array::array_elements_ptr(keys).cast(), 0, i),
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
                            slot(crate::array::array_elements_ptr(arr).cast(), 0, j),
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
    });
    drop(construction);
    if large_output {
        super::stringify_flat::note_completed_malloc_json_output(bytes);
    }
    value
}

#[cfg(test)]
#[path = "stringify_record_output_tests.rs"]
mod tests;
