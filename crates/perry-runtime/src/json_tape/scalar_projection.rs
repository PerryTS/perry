//! Read an own scalar field without exposing/materializing the containing record.
//! This probe never allocates in the managed heap, polls, or invokes user code.
//! A miss leaves ordinary indexing/property access responsible for all semantics.

use super::*;

// A probe must not turn random reads or wide records into repeated full walks.
// Larger work and repeated indices use ordinary materialization/caching.
const MAX_INDEX_STEPS: u32 = 32;
const MAX_RECORD_FIELDS: usize = 32;

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
    if (*hdr).magic != LAZY_ARRAY_MAGIC
        || !(*hdr).materialized.is_null()
        || index >= (*hdr).cached_length
        || index == (*hdr).walk_idx
    {
        return None;
    }
    let bitmap = (*hdr).materialized_bitmap;
    if !bitmap.is_null() && *bitmap.add(index as usize / 64) & (1u64 << (index % 64)) != 0 {
        // A previously exposed record can have mutations, getters, a changed
        // prototype or escaped child identities. Its current object wins.
        return None;
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
    // Scalar cursor stores only: no new element cache entry or GC edge. A
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
    name_len: usize,
) -> f64 {
    let miss = f64::from_bits(crate::value::TAG_HOLE);
    let bits = receiver.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG || name.is_null() {
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
    let name = std::slice::from_raw_parts(name, name_len);
    if !name.is_ascii() || name.contains(&b'"') || name.contains(&b'\\') {
        return miss;
    }
    project(raw as *mut LazyArrayHeader, index as u32, name)
        .map_or(miss, |value| f64::from_bits(value.bits()))
}

#[cfg(test)]
#[path = "scalar_projection_tests.rs"]
mod tests;
