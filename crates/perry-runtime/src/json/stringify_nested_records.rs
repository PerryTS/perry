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
            // add_plan records these offsets only after appending complete
            // escaped UTF-8 keys. Plans and the private pool cannot change
            // during this copy; neither offset needs validating again.
            let prefix = self
                .prefixes
                .get_unchecked(plan.offsets[i] as usize..plan.offsets[i + 1] as usize);
            append_text(buf, prefix);
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

/// Copy at most 32 bytes using exact in-bounds loads and stores. The first
/// and last words may overlap within their own ranges; no padding is read or
/// written. The caller provides disjoint source/destination ranges of `len`
/// bytes, and reserves output before deriving the destination pointer.
#[inline(always)]
unsafe fn copy_short(src: *const u8, dst: *mut u8, len: usize) {
    debug_assert!(len <= 32);
    if len >= 16 {
        dst.cast::<u128>()
            .write_unaligned(src.cast::<u128>().read_unaligned());
        dst.add(len - 16)
            .cast::<u128>()
            .write_unaligned(src.add(len - 16).cast::<u128>().read_unaligned());
    } else if len >= 8 {
        dst.cast::<u64>()
            .write_unaligned(src.cast::<u64>().read_unaligned());
        dst.add(len - 8)
            .cast::<u64>()
            .write_unaligned(src.add(len - 8).cast::<u64>().read_unaligned());
    } else if len >= 4 {
        dst.cast::<u32>()
            .write_unaligned(src.cast::<u32>().read_unaligned());
        dst.add(len - 4)
            .cast::<u32>()
            .write_unaligned(src.add(len - 4).cast::<u32>().read_unaligned());
    } else if len >= 2 {
        dst.cast::<u16>()
            .write_unaligned(src.cast::<u16>().read_unaligned());
        dst.add(len - 2)
            .cast::<u16>()
            .write_unaligned(src.add(len - 2).cast::<u16>().read_unaligned());
    } else if len == 1 {
        dst.write(src.read());
    }
}

#[inline(always)]
fn append_text(buf: &mut String, text: &str) {
    let len = text.len();
    if len > 32 {
        buf.push_str(text);
        return;
    }
    // text is valid UTF-8 and cannot alias the mutably borrowed String.
    // reserve checks capacity/overflow before pointers or length are changed.
    unsafe {
        let bytes = buf.as_mut_vec();
        bytes.reserve(len);
        let start = bytes.len();
        copy_short(text.as_ptr(), bytes.as_mut_ptr().add(start), len);
        bytes.set_len(start + len);
    }
}

#[inline]
unsafe fn emit_number(value: f64, buf: &mut String) {
    let integer = value as i32;
    // Equality after the saturating conversion proves an exact i32. This
    // includes -0 -> "0"; fractions, larger numbers, NaNs and tagged int32s
    // retain the existing ECMAScript-compatible general formatter.
    if value == integer as f64 {
        let mut digits = itoa::Buffer::new();
        append_text(buf, digits.format(integer));
    } else {
        write_number(buf, value);
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
                if scratch[..len]
                    .iter()
                    .all(|&b| (0x20..=0x7f).contains(&b) && b != b'"' && b != b'\\')
                {
                    // The byte check proves both valid ASCII and no escaping.
                    // Assemble quotes in bounded native storage, then append
                    // once without a second UTF-8/escape scan.
                    let mut quoted = [b'"'; crate::value::SHORT_STRING_MAX_LEN + 2];
                    copy_short(scratch.as_ptr(), quoted.as_mut_ptr().add(1), len);
                    append_text(buf, std::str::from_utf8_unchecked(&quoted[..len + 2]));
                } else if let Ok(text) = std::str::from_utf8(&scratch[..len]) {
                    write_escaped_string(buf, text);
                } else {
                    buf.push_str("null");
                }
            }
            _ => emit_number(f64::from_bits(bits), buf),
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
