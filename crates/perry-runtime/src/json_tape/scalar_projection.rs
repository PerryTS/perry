//! Read an own scalar field without exposing/materializing the containing record.
//! This probe never allocates in the managed heap, polls, or invokes user code.
//! A miss leaves ordinary indexing/property access responsible for all semantics.

use super::*;

// A probe must not turn random reads or wide records into repeated full walks.
// Larger work and repeated indices use ordinary materialization/caching.
const MAX_INDEX_STEPS: u32 = 32;
const MAX_RECORD_FIELDS: usize = 32;

/// Exact, collision-free encoding of up to seven ASCII bytes. The low byte is
/// length + 1, so even the empty property differs from the unchosen key (zero).
fn property_id(name: &[u8]) -> Option<u64> {
    if name.len() > 7 || !name.is_ascii() || name.contains(&b'"') || name.contains(&b'\\') {
        return None;
    }
    let mut bytes = [0; 8];
    bytes[0] = name.len() as u8 + 1;
    bytes[1..name.len() + 1].copy_from_slice(name);
    Some(u64::from_le_bytes(bytes))
}

// Slots already start zeroed. Encode +0 as the pointer-free tagged integer 0
// so an empty slot remains distinguishable, without another bitmap or an
// initialization pass. JSON number materialization itself emits f64 values.
const MEMO_ZERO: u64 = crate::value::INT32_TAG;

fn memo_decode(bits: u64) -> JSValue {
    JSValue::from_bits(if bits == MEMO_ZERO { 0 } else { bits })
}

// The 64-bit inline read in codegen's index_get/scalar_projection.rs uses these
// offsets. Other targets retain ordinary indexing; the runtime ABI is u64.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::offset_of!(LazyArrayHeader, magic) == 4);
    assert!(std::mem::offset_of!(LazyArrayHeader, materialized) == 32);
    assert!(std::mem::offset_of!(LazyArrayHeader, materialized_elements) == 40);
    assert!(std::mem::offset_of!(LazyArrayHeader, materialized_bitmap) == 48);
    assert!(std::mem::offset_of!(LazyArrayHeader, scalar_property) == 80);
};

/// A cursor can name either an exposed record or an unmaterialized projection.
/// In the latter case a preserved streak belongs to the previous cold element:
/// materializing this very record completes the next step of that run.
pub(super) unsafe fn continues_materialization_run(
    hdr: *mut LazyArrayHeader,
    index: u32,
    previous: u32,
) -> bool {
    if previous == u32::MAX || previous >= (*hdr).cached_length {
        return false;
    }
    let bitmap = (*hdr).materialized_bitmap;
    let previous_cached =
        !bitmap.is_null() && *bitmap.add(previous as usize / 64) & (1u64 << (previous % 64)) != 0;
    if previous_cached {
        index == previous + 1
    } else {
        index == previous && (*hdr).sequential_streak != 0
    }
}

/// Compare an encoded JSON key with a plain ASCII property name. A differing
/// plain byte proves a miss immediately; an escape before that proof declines
/// the projection so escaped duplicate keys retain last-property-wins behavior.
fn key_matches(bytes: &[u8], name: &[u8]) -> Option<bool> {
    if bytes.first() != Some(&b'"') {
        return None;
    }
    for (i, expected) in name.iter().enumerate() {
        let byte = *bytes.get(i + 1)?;
        if byte == b'\\' {
            return None;
        }
        if byte != *expected {
            return Some(false);
        }
    }
    match bytes.get(name.len() + 1)? {
        b'\\' => None,
        b'"' => Some(true),
        _ => Some(false),
    }
}

unsafe fn record_scalar(source: &TapeSource<'_, '_>, start: usize, name: &[u8]) -> Option<JSValue> {
    let record = source.entry(start)?;
    if record.kind != KIND_OBJ_START {
        return None;
    }
    let end = record.link as usize;
    let mut position = start + 1;
    let mut selected = None;
    let mut fields = 0;
    while position < end {
        if fields == MAX_RECORD_FIELDS {
            return None;
        }
        fields += 1;
        let key = source.entry(position)?;
        if key.kind != KIND_KEY {
            return None;
        }
        let value = source.entry(position + 1)?;
        if key_matches(source.bytes_from_offset(key.offset as usize), name)? {
            selected = Some(value);
        }
        position = match value.kind {
            KIND_OBJ_START | KIND_ARR_START => value.link as usize + 1,
            _ => position + 2,
        };
    }
    let value = selected?;
    Some(match value.kind {
        KIND_NUMBER => materialize_number(source, value.offset as usize),
        KIND_TRUE => JSValue::bool(true),
        KIND_FALSE => JSValue::bool(false),
        KIND_NULL => JSValue::null(),
        // Strings require construction/rooting; containers require identity.
        // Missing fields may resolve to a user getter on Object.prototype.
        _ => return None,
    })
}

unsafe fn project(hdr: *mut LazyArrayHeader, index: u32, name: &[u8]) -> Option<JSValue> {
    let property = property_id(name)?;
    if (*hdr).magic != LAZY_ARRAY_MAGIC
        || !(*hdr).materialized.is_null()
        || index >= (*hdr).cached_length
    {
        return None;
    }
    let bitmap = (*hdr).materialized_bitmap;
    if !bitmap.is_null() && *bitmap.add(index as usize / 64) & (1u64 << (index % 64)) != 0 {
        // An exposed record (including its mutations/getters) always wins.
        return None;
    }
    let cache = (*hdr).materialized_elements;
    if cache.is_null() || bitmap.is_null() {
        return None;
    }
    if (*hdr).scalar_property != 0 {
        if (*hdr).scalar_property != property {
            return None;
        }
        let memo = (*cache.add(index as usize)).bits();
        if memo != 0 {
            return Some(memo_decode(memo));
        }
    }
    let source = TapeSource::Borrowed {
        tape: LazyArrayHeader::tape_slice(hdr),
        bytes: LazyArrayHeader::blob_bytes(hdr),
    };
    let root = (*hdr).root_idx as usize;
    let root_entry = source.entry(root)?;
    if root_entry.kind != KIND_ARR_START {
        return None;
    }
    let end = root_entry.link as usize;
    let (mut count, mut position) = if (*hdr).walk_idx != u32::MAX && index >= (*hdr).walk_idx {
        ((*hdr).walk_idx, (*hdr).walk_tape_pos as usize)
    } else {
        (0, root + 1)
    };
    if index - count > MAX_INDEX_STEPS {
        return None;
    }
    while count < index && position < end {
        let entry = source.entry(position)?;
        position = match entry.kind {
            KIND_OBJ_START | KIND_ARR_START => entry.link as usize + 1,
            _ => position + 1,
        };
        count += 1;
    }
    if position >= end {
        return None;
    }
    let result = record_scalar(&source, position, name)?;
    (*hdr).scalar_property = property;
    let bits = result.bits();
    // GC_STORE_AUDIT(POINTER_FREE): number/bool/null only, bitmap remains clear.
    *cache.add(index as usize) = JSValue::from_bits(if bits == 0 { MEMO_ZERO } else { bits });
    // Scalar memo/cursor stores only: no exposed element or GC edge. A
    // projection is not a cold materialization and must not extend its streak.
    // Preserve a run only when this projection immediately follows an exposed
    // record. If ordinary access now materializes this same index, it continues
    // that run; projecting a second unexposed record instead resets it. This
    // keeps mixed id/name reads eligible for the existing batch producer.
    // The hard walk bound above keeps random access from bypassing the ordinary
    // path's adaptive materialization budget with an unbounded tape traversal.
    if !continues_materialization_run(hdr, index, (*hdr).walk_idx) {
        (*hdr).sequential_streak = 0;
    }
    (*hdr).walk_idx = index;
    (*hdr).walk_tape_pos = position as u32;
    Some(result)
}

/// Probe `receiver[index].name` for a pristine lazy JSON record. TAG_HOLE means
/// "use the ordinary expression" and can never be a successful JSON scalar.
///
/// # Safety
/// `name` must point to `name_len` readable bytes for this call. Codegen supplies
/// a static ASCII literal. Boxed receiver/index arguments follow the ordinary
/// runtime ABI. No borrowed heap pointer survives this noncollecting call.
#[no_mangle]
pub unsafe extern "C" fn js_json_lazy_index_scalar(
    receiver: f64,
    index: f64,
    name: *const u8,
    name_len: u64,
) -> f64 {
    let miss = f64::from_bits(crate::value::TAG_HOLE);
    let bits = receiver.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG || name.is_null() || name_len > 7
    {
        return miss;
    }
    let index = if index.to_bits() & crate::value::TAG_MASK == crate::value::INT32_TAG {
        JSValue::from_bits(index.to_bits()).as_int32() as f64
    } else {
        index
    };
    if !index.is_finite() || index < 0.0 || index.fract() != 0.0 || index >= u32::MAX as f64 {
        return miss;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
        return miss;
    };
    if header.obj_type != crate::gc::GC_TYPE_LAZY_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
            != 0
    {
        return miss;
    }
    let name = std::slice::from_raw_parts(name, name_len as usize);
    if !name.is_ascii() || name.contains(&b'"') || name.contains(&b'\\') {
        return miss;
    }
    project(raw as *mut LazyArrayHeader, index as u32, name)
        .map_or(miss, |value| f64::from_bits(value.bits()))
}

/// One-time admission probe for an invariant `array[index].name` numeric read.
/// Returns a genuine f64 or TAG_HOLE. It never invokes JS, coerces a value,
/// materializes a record, allocates in Perry's heap, polls, or throws. The only
/// nontrivial callees are the tape projection above and the GC-leaf own-field
/// reader. Descriptor-backed and declared-class records deliberately decline:
/// a raw slot need not represent their ordinary property access semantics.
///
/// # Safety
/// `name` points to `name_len` readable bytes. `receiver` follows the ordinary
/// boxed-value ABI. Borrowed edges never outlive this noncollecting call.
#[no_mangle]
pub unsafe extern "C" fn js_array_index_own_number(
    receiver: f64,
    index: u32,
    name: *const u8,
    name_len: u64,
) -> f64 {
    let miss = f64::from_bits(crate::value::TAG_HOLE);
    let Ok(name_len) = usize::try_from(name_len) else {
        return miss;
    };
    if name.is_null() || index == u32::MAX {
        return miss;
    }
    let bits = receiver.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return miss;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
        return miss;
    };
    let forbidden = crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 || header._reserved & forbidden != 0 {
        return miss;
    }
    let element = if header.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        let hdr = raw as *mut LazyArrayHeader;
        if (*hdr).magic != LAZY_ARRAY_MAGIC {
            return miss;
        }
        if !(*hdr).materialized.is_null() {
            // The backing array's own live length is authoritative after alias
            // mutations. Forwarded backings decline without repairing an edge.
            let Some(value) = dense_element((*hdr).materialized, index) else {
                return miss;
            };
            value
        } else {
            if index >= (*hdr).cached_length
                || (*hdr).materialized_bitmap.is_null()
                || (*hdr).materialized_elements.is_null()
            {
                return miss;
            }
            let exposed =
                *(*hdr).materialized_bitmap.add(index as usize / 64) & (1u64 << (index % 64)) != 0;
            if exposed {
                *(*hdr).materialized_elements.add(index as usize)
            } else {
                let name = std::slice::from_raw_parts(name, name_len);
                return project(hdr, index, name).map_or(miss, own_number);
            }
        }
    } else if header.obj_type == crate::gc::GC_TYPE_ARRAY {
        let Some(value) = dense_element(raw as *const crate::array::ArrayHeader, index) else {
            return miss;
        };
        value
    } else {
        return miss;
    };
    if !element.is_pointer() {
        return miss;
    }
    let obj = element.as_pointer::<crate::object::ObjectHeader>();
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return miss;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & (forbidden | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO) != 0
        || *((obj as *const u8).add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32)
            == crate::closure::CLOSURE_MAGIC
        || (*obj).class_id != 0
        || !crate::object::object_is_regular(obj)
    {
        return miss;
    }
    let value = crate::object::js_object_get_own_field_or_undef(
        f64::from_bits(element.bits()),
        name,
        name_len,
    );
    own_number(JSValue::from_bits(value.to_bits()))
}

fn own_number(value: JSValue) -> f64 {
    if value.is_int32() {
        value.as_int32() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        f64::from_bits(crate::value::TAG_HOLE)
    }
}

unsafe fn dense_element(arr: *const crate::array::ArrayHeader, index: u32) -> Option<JSValue> {
    let header = crate::value::addr_class::try_read_gc_header(arr as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
            != 0
        || index >= (*arr).length
        || (*arr).length > (*arr).capacity
    {
        return None;
    }
    let slots =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const JSValue;
    Some(*slots.add(index as usize))
}

#[cfg(test)]
#[path = "scalar_projection_tests.rs"]
mod tests;
