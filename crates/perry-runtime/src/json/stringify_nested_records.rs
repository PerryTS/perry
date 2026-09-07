//! Native-only traversal of ordinary records with primitive object leaves.
//!
//! The complete attempt contains no managed allocation or user callback. Key
//! prefixes are bounded native scratch, dropped before any general fallback.
//! Their raw identities and negative toJSON proofs cannot cross a collection
//! or mutation boundary. Values and results are never cached across calls.

use super::*;
use std::mem::MaybeUninit;

const MAX_FIELDS: usize = 32;
const MAX_SHAPES: usize = 64;
const MAX_PREFIX_BYTES: usize = 64 * 1024;
const PLAN_BUCKETS: usize = MAX_SHAPES * 2;

struct Record {
    values: *const u64,
    len: usize,
}

/// Per-instance facts cannot be inferred from a cached shape. In particular,
/// an object's descriptors, explicit prototype and allocation bounds must be
/// checked even when its sibling already supplied a key plan.
unsafe fn ordinary_object(bits: u64) -> Option<(*const crate::ObjectHeader, usize)> {
    if bits & crate::value::TAG_MASK != POINTER_TAG {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *const crate::ObjectHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(obj as usize)?.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_PLAIN_ORDINARY == 0
        || header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>()
        || (*obj).class_id != 0
        || crate::object::prototype_chain::object_static_prototype(obj as usize).is_some()
    {
        return None;
    }
    Some((obj, header.size as usize))
}

unsafe fn record(bits: u64) -> Option<Record> {
    let (obj, size) = ordinary_object(bits)?;
    let descriptor = crate::object::shapes::shape_descriptor_by_id(
        crate::object::shapes::object_shape_stamp(obj),
    )?;
    let keys = descriptor.keys as usize as *const crate::ArrayHeader;
    let len = key_count(keys)?;
    if len > descriptor.live_inline_slot_count as usize
        || size < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>() + len * 8
    {
        return None;
    }
    Some(Record {
        values: (obj as *const u8)
            .add(std::mem::size_of::<crate::ObjectHeader>())
            .cast(),
        len,
    })
}

unsafe fn key_count(keys: *const crate::ArrayHeader) -> Option<usize> {
    let len = if keys.is_null() {
        0
    } else {
        let kh = crate::value::addr_class::try_read_tracked_gc_header(keys as usize)?.as_ref();
        if kh.obj_type != crate::gc::GC_TYPE_ARRAY
            || kh.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || (kh.size as usize)
                < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ArrayHeader>()
            || (*keys).length > (*keys).capacity
        {
            return None;
        }
        let len = (*keys).length as usize;
        if len > MAX_FIELDS
            || (kh.size as usize)
                < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ArrayHeader>() + len * 8
        {
            return None;
        }
        len
    };
    Some(len)
}

#[derive(Clone, Copy)]
struct KeyPlan {
    shape: u32,
    len: u32,
    offsets: [u32; MAX_FIELDS + 1],
}

struct Emitter {
    // Zero is an empty bucket; occupied buckets contain plan index + 1. The
    // table is at most half full, so probing always reaches an empty bucket.
    indices: [u8; PLAN_BUCKETS],
    plans: [MaybeUninit<KeyPlan>; MAX_SHAPES],
    count: usize,
    prefixes: String,
}

impl Emitter {
    fn new() -> Self {
        Self {
            indices: [0; PLAN_BUCKETS],
            plans: [MaybeUninit::uninit(); MAX_SHAPES],
            count: 0,
            prefixes: String::new(),
        }
    }

    fn get(&self, index: usize) -> &KeyPlan {
        debug_assert!(index < self.count);
        // Only completed plans are counted or published into indices.
        unsafe { self.plans[index].assume_init_ref() }
    }

    unsafe fn plan(&mut self, object: *const crate::ObjectHeader) -> Option<usize> {
        let shape = crate::object::shapes::object_shape_stamp(object);
        if shape == 0 {
            return None;
        }
        let mut bucket = (shape.wrapping_mul(0x9e37_79b9) >> 25) as usize;
        while self.indices[bucket] != 0 {
            let index = (self.indices[bucket] - 1) as usize;
            if self.get(index).shape == shape {
                return Some(index);
            }
            bucket = (bucket + 1) & (PLAN_BUCKETS - 1);
        }
        self.add_plan(object, shape, bucket)
    }

    #[inline(never)]
    unsafe fn add_plan(
        &mut self,
        object: *const crate::ObjectHeader,
        shape: u32,
        bucket: usize,
    ) -> Option<usize> {
        let descriptor = crate::object::shapes::shape_descriptor_by_id(shape)?;
        let keys_array = descriptor.keys as usize as *const crate::ArrayHeader;
        let len = key_count(keys_array)?;
        if self.count == MAX_SHAPES
            || len > descriptor.live_inline_slot_count as usize
            || !super::stringify_tojson_probe::to_json_definitely_absent_without_gc(object.cast())
            || crate::object::keys_contain_array_index(keys_array)
        {
            return None;
        }
        // ShapeIds include the key layout and live-slot bound and are never
        // reused. Their validated key facts stay valid during this attempt:
        // no managed allocation, callback, mutation or collection can occur.
        let mut plan = KeyPlan {
            shape,
            len: len as u32,
            offsets: [0; MAX_FIELDS + 1],
        };
        plan.offsets[0] = self.prefixes.len() as u32;
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let keys = (keys_array as *const u8)
            .wrapping_add(std::mem::size_of::<crate::ArrayHeader>())
            .cast::<u64>();
        for i in 0..len {
            let bytes =
                crate::string::js_string_key_bytes(JSValue::from_bits(*keys.add(i)), &mut scratch)?;
            // Account for worst-case escaping before growing native scratch.
            let remaining = MAX_PREFIX_BYTES.checked_sub(self.prefixes.len())?;
            if bytes.len().checked_mul(6)?.checked_add(4)? > remaining {
                return None;
            }
            let key = std::str::from_utf8(bytes).ok()?;
            self.prefixes.push(if i == 0 { '{' } else { ',' });
            write_escaped_string(&mut self.prefixes, key);
            self.prefixes.push(':');
            plan.offsets[i + 1] = self.prefixes.len() as u32;
        }
        let index = self.count;
        self.plans[index].write(plan);
        self.count += 1;
        self.indices[bucket] = (index + 1) as u8;
        Some(index)
    }

    unsafe fn emit_record(&mut self, bits: u64, allow_child: bool, buf: &mut String) -> Option<()> {
        let (object, size) = ordinary_object(bits)?;
        let index = self.plan(object)?;
        let len = self.get(index).len as usize;
        if size < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>() + len * 8 {
            return None;
        }
        if len == 0 {
            buf.push_str("{}");
            return Some(());
        }
        let values = (object as *const u8)
            .add(std::mem::size_of::<crate::ObjectHeader>())
            .cast::<u64>();
        for i in 0..len {
            let value = *values.add(i);
            // Omitted fields require different comma placement; decline before
            // invoking the general serializer, which owns all omission rules.
            if value == TAG_UNDEFINED {
                return None;
            }
            let plan = self.get(index);
            buf.push_str(&self.prefixes[plan.offsets[i] as usize..plan.offsets[i + 1] as usize]);
            if super::stringify_primitive_object::field_is_primitive(value) {
                emit_primitive(value, buf);
            } else if allow_child {
                self.emit_record(value, false, buf)?;
            } else {
                return None;
            }
        }
        buf.push('}');
        Some(())
    }
}

unsafe fn emit_primitive(bits: u64, buf: &mut String) {
    match bits {
        TAG_NULL => buf.push_str("null"),
        TAG_TRUE => buf.push_str("true"),
        TAG_FALSE => buf.push_str("false"),
        _ => match bits & crate::value::TAG_MASK {
            STRING_TAG => {
                let ptr = (bits & POINTER_MASK) as *const StringHeader;
                if let Some(text) = str_from_header(ptr) {
                    write_escaped_string(buf, text);
                } else {
                    buf.push_str("null");
                }
            }
            crate::value::SHORT_STRING_TAG => {
                let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
                let len = JSValue::from_bits(bits).short_string_to_buf(&mut scratch);
                if let Ok(text) = std::str::from_utf8(&scratch[..len]) {
                    write_escaped_string(buf, text);
                } else {
                    buf.push_str("null");
                }
            }
            _ => write_number(buf, f64::from_bits(bits)),
        },
    }
}

/// The caller has resolved the array, applied its toJSON, checked for a cycle,
/// and rejected exotic indexing/prototypes. A false result restores the output
/// length and leaves serializer state untouched; native capacity may have grown.
#[inline(never)]
pub(super) unsafe fn try_emit(
    arr: *const crate::ArrayHeader,
    buf: &mut String,
    depth: u32,
) -> bool {
    let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(arr as usize) else {
        return false;
    };
    let header = header.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*arr).length < 2
        || (*arr).length > (*arr).capacity
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE
                + std::mem::size_of::<crate::ArrayHeader>()
                + (*arr).length as usize * 8
        || depth as usize > super::stringify::MAX_STRINGIFY_NESTING_DEPTH - 3
        || SUPPRESS_NEXT_TO_JSON.with(|c| c.get())
    {
        return false;
    }
    let elements = (arr as *const u8)
        .add(std::mem::size_of::<crate::ArrayHeader>())
        .cast::<u64>();
    let Some(first) = record(*elements) else {
        return false;
    };
    // Leave primitive records and primitive-array fields on their existing
    // paths. This route amortizes per-shape work for nested ordinary records.
    if !(0..first.len).any(|i| record(*first.values.add(i)).is_some()) {
        return false;
    }
    emit_array(arr, elements, buf)
}

// Keep the bounded plan frame off the stack until the first-record guard has
// established eligibility. Other array workloads retain the cheap decline.
#[inline(never)]
unsafe fn emit_array(
    arr: *const crate::ArrayHeader,
    elements: *const u64,
    buf: &mut String,
) -> bool {
    let saved = buf.len();
    let mut emitter = Emitter::new();
    buf.push('[');
    for i in 0..(*arr).length as usize {
        if i != 0 {
            buf.push(',');
        }
        if emitter.emit_record(*elements.add(i), true, buf).is_none() {
            buf.truncate(saved);
            return false;
        }
    }
    buf.push(']');
    true
}

#[cfg(test)]
#[path = "stringify_nested_records_tests.rs"]
mod tests;
