//! Native-only traversal of ordinary records with primitive object leaves.
//!
//! The complete attempt contains no managed allocation or user callback. Key
//! prefixes are bounded native scratch, dropped before any general fallback.
//! Their raw identities and negative toJSON proofs cannot cross a collection
//! or mutation boundary. Values and results are never cached across calls.

use super::*;
use std::collections::HashMap;

const MAX_FIELDS: usize = 32;
const MAX_SHAPES: usize = 64;
const MAX_PREFIX_BYTES: usize = 64 * 1024;

struct Record {
    object: *const crate::ObjectHeader,
    keys: *const crate::ArrayHeader,
    values: *const u64,
    len: usize,
}

unsafe fn record(bits: u64) -> Option<Record> {
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
    let keys = crate::object::object_keys_array(obj);
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
    if len > crate::object::object_live_slot_count(obj) as usize
        || (header.size as usize)
            < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::ObjectHeader>() + len * 8
    {
        return None;
    }
    Some(Record {
        object: obj,
        keys,
        values: (obj as *const u8)
            .add(std::mem::size_of::<crate::ObjectHeader>())
            .cast(),
        len,
    })
}

struct KeyPlan {
    prefix: String,
    ends: [u32; MAX_FIELDS],
}

#[derive(Default)]
struct Emitter {
    indices: HashMap<usize, usize>,
    plans: Vec<KeyPlan>,
    prefix_bytes: usize,
}

impl Emitter {
    unsafe fn plan(&mut self, record: &Record) -> Option<usize> {
        if let Some(&index) = self.indices.get(&(record.keys as usize)) {
            return Some(index);
        }
        if self.plans.len() == MAX_SHAPES
            || !super::stringify_tojson_probe::to_json_definitely_absent_without_gc(
                record.object.cast(),
            )
            || crate::object::keys_contain_array_index(record.keys)
        {
            return None;
        }
        let mut plan = KeyPlan {
            prefix: String::new(),
            ends: [0; MAX_FIELDS],
        };
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let keys = (record.keys as *const u8)
            .wrapping_add(std::mem::size_of::<crate::ArrayHeader>())
            .cast::<u64>();
        for i in 0..record.len {
            let bytes =
                crate::string::js_string_key_bytes(JSValue::from_bits(*keys.add(i)), &mut scratch)?;
            // Account for worst-case escaping before growing native scratch.
            let remaining = MAX_PREFIX_BYTES.checked_sub(self.prefix_bytes + plan.prefix.len())?;
            if bytes.len().checked_mul(6)?.checked_add(4)? > remaining {
                return None;
            }
            let key = std::str::from_utf8(bytes).ok()?;
            plan.prefix.push(if i == 0 { '{' } else { ',' });
            write_escaped_string(&mut plan.prefix, key);
            plan.prefix.push(':');
            plan.ends[i] = plan.prefix.len() as u32;
        }
        let index = self.plans.len();
        self.prefix_bytes += plan.prefix.len();
        self.plans.push(plan);
        self.indices.insert(record.keys as usize, index);
        Some(index)
    }

    unsafe fn emit_record(&mut self, bits: u64, allow_child: bool, buf: &mut String) -> Option<()> {
        let record = record(bits)?;
        let index = self.plan(&record)?;
        if record.len == 0 {
            buf.push_str("{}");
            return Some(());
        }
        let mut start = 0;
        for i in 0..record.len {
            let value = *record.values.add(i);
            // Omitted fields require different comma placement; decline before
            // invoking the general serializer, which owns all omission rules.
            if value == TAG_UNDEFINED {
                return None;
            }
            let plan = &self.plans[index];
            let end = plan.ends[i] as usize;
            buf.push_str(&plan.prefix[start..end]);
            start = end;
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
    let saved = buf.len();
    let mut emitter = Emitter::default();
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
