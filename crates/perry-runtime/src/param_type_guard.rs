//! Non-throwing runtime validation for ordinary-parameter specialized clones.
//!
//! The descriptors consumed here are immutable compiler-emitted byte graphs.
//! This helper deliberately performs no JavaScript operations: it does not
//! coerce values, follow prototypes, invoke accessors, or enter user code. A
//! shape it cannot validate cheaply and directly simply takes the generic
//! function fallback.

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::value::{JSValue, POINTER_MASK, TAG_FALSE, TAG_HOLE, TAG_TRUE};
use std::collections::HashSet;

const MAGIC: u32 = 0x3154_4750; // `PGT1`, little-endian.
const MAX_DESCRIPTOR_LEN: usize = 1 << 20;
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 256;
const MAX_CONTAINER_LEN: usize = 16_000_000;
const INLINE_VISITED: usize = 64;

const OP_ANY: u8 = 0;
const OP_NUMBER: u8 = 1;
const OP_INT32: u8 = 2;
const OP_BOOLEAN: u8 = 3;
const OP_STRING: u8 = 4;
const OP_NULL: u8 = 5;
const OP_UNDEFINED: u8 = 6;
const OP_BIGINT: u8 = 7;
const OP_SYMBOL: u8 = 8;
const OP_ARRAY: u8 = 9;
const OP_TUPLE: u8 = 10;
const OP_OBJECT: u8 = 11;
const OP_UNION: u8 = 12;
const OP_STRING_LITERAL: u8 = 13;
const OP_RECURSIVE_REF: u8 = 14;
const OP_MAP: u8 = 15;
const OP_SET: u8 = 16;

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

struct Descriptor<'a> {
    bytes: &'a [u8],
    root: u32,
    node_count: usize,
}

impl<'a> Descriptor<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 16 || bytes.len() > MAX_DESCRIPTOR_LEN || read_u32(bytes, 0)? != MAGIC {
            return None;
        }
        let root = read_u32(bytes, 4)?;
        let node_count = read_u32(bytes, 8)? as usize;
        if node_count == 0 || node_count > MAX_NODES || root as usize >= node_count {
            return None;
        }
        let table_end = 12usize.checked_add((node_count + 1).checked_mul(4)?)?;
        if table_end > bytes.len() {
            return None;
        }
        if read_u32(bytes, 12)? as usize != table_end
            || read_u32(bytes, 12 + node_count * 4)? as usize != bytes.len()
        {
            return None;
        }
        Some(Self {
            bytes,
            root,
            node_count,
        })
    }

    fn node(&self, id: u32) -> Option<&'a [u8]> {
        let index = id as usize;
        if index >= self.node_count {
            return None;
        }
        let start = read_u32(self.bytes, 12 + index * 4)? as usize;
        let end = read_u32(self.bytes, 12 + (index + 1) * 4)? as usize;
        let table_end = 12 + (self.node_count + 1) * 4;
        if start < table_end || end < start {
            return None;
        }
        self.bytes.get(start..end).filter(|node| !node.is_empty())
    }
}

struct GuardState<'a> {
    descriptor: Descriptor<'a>,
    /// Container/node pairs already proved during this traversal. The log
    /// makes speculative union arms reversible: a failed arm must not leave
    /// behind a fact that could make a later recursive visit succeed.
    inline_visited: [(usize, u32); INLINE_VISITED],
    inline_visited_len: usize,
    spill_visited: Option<HashSet<(usize, u32)>>,
    spill_log: Vec<(usize, u32)>,
}

enum OwnField {
    Missing,
    Data(JSValue),
    Invalid,
}

impl GuardState<'_> {
    fn checkpoint(&self) -> usize {
        self.inline_visited_len + self.spill_log.len()
    }

    fn seen_or_insert(&mut self, address: usize, node_id: u32) -> bool {
        let key = (address, node_id);
        if self.inline_visited[..self.inline_visited_len].contains(&key)
            || self
                .spill_visited
                .as_ref()
                .is_some_and(|visited| visited.contains(&key))
        {
            return true;
        }
        if self.inline_visited_len < INLINE_VISITED {
            self.inline_visited[self.inline_visited_len] = key;
            self.inline_visited_len += 1;
        } else {
            self.spill_visited
                .get_or_insert_with(HashSet::new)
                .insert(key);
            self.spill_log.push(key);
        }
        false
    }

    fn rollback(&mut self, checkpoint: usize) {
        while self.checkpoint() > checkpoint {
            if let Some(key) = self.spill_log.pop() {
                if let Some(visited) = &mut self.spill_visited {
                    visited.remove(&key);
                }
            } else {
                self.inline_visited_len -= 1;
            }
        }
    }

    unsafe fn plain_array(&self, value: JSValue) -> Option<(*const ArrayHeader, usize)> {
        if !value.is_pointer() {
            return None;
        }
        let address = (value.bits() & POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(address)?;
        if header.obj_type != crate::gc::GC_TYPE_ARRAY
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        {
            return None;
        }
        let array = address as *const ArrayHeader;
        let length = (*array).length as usize;
        let capacity = (*array).capacity as usize;
        let required = crate::gc::GC_HEADER_SIZE
            .checked_add(std::mem::size_of::<ArrayHeader>())?
            .checked_add(capacity.checked_mul(std::mem::size_of::<f64>())?)?;
        if length > capacity || length > MAX_CONTAINER_LEN || required > header.size as usize {
            return None;
        }
        Some((array, length))
    }

    unsafe fn plain_object(&self, value: JSValue) -> Option<(*const ObjectHeader, usize)> {
        if !value.is_pointer() {
            return None;
        }
        let address = (value.bits() & POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(address)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let object = address as *const ObjectHeader;
        // #8113: the header no longer carries `object_type` / `field_count`;
        // the receiver kind comes from the ShapeId descriptor and the live
        // inline-slot bound from `object_live_slot_count`.
        if !crate::object::object_is_regular(object) {
            return None;
        }
        let live_slots = crate::object::object_live_slot_count(object) as usize;
        if live_slots > MAX_CONTAINER_LEN {
            return None;
        }
        let inline_fields = live_slots.max(crate::object::INLINE_SLOT_FLOOR);
        let required = crate::gc::GC_HEADER_SIZE
            .checked_add(std::mem::size_of::<ObjectHeader>())?
            .checked_add(inline_fields.checked_mul(std::mem::size_of::<JSValue>())?)?;
        if required > header.size as usize {
            return None;
        }
        Some((object, address))
    }

    unsafe fn plain_map(&self, value: JSValue) -> Option<(*const crate::map::MapHeader, usize)> {
        if !value.is_pointer() {
            return None;
        }
        let address = (value.bits() & POINTER_MASK) as usize;
        if !crate::map::is_registered_map(address) {
            return None;
        }
        let header = crate::value::addr_class::try_read_gc_header(address)?;
        if header.obj_type != crate::gc::GC_TYPE_MAP
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let map = address as *const crate::map::MapHeader;
        let size = (*map).size as usize;
        let capacity = (*map).capacity as usize;
        if size > capacity || size > MAX_CONTAINER_LEN || (size != 0 && (*map).entries.is_null()) {
            return None;
        }
        Some((map, size))
    }

    unsafe fn plain_set(&self, value: JSValue) -> Option<(*const crate::set::SetHeader, usize)> {
        if !value.is_pointer() {
            return None;
        }
        let address = (value.bits() & POINTER_MASK) as usize;
        if !crate::set::is_registered_set(address) {
            return None;
        }
        let header = crate::value::addr_class::try_read_gc_header(address)?;
        if header.obj_type != crate::gc::GC_TYPE_SET
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let set = address as *const crate::set::SetHeader;
        let size = (*set).size as usize;
        let capacity = (*set).capacity as usize;
        if size > capacity || size > MAX_CONTAINER_LEN || (size != 0 && (*set).elements.is_null()) {
            return None;
        }
        Some((set, size))
    }

    unsafe fn own_data_field(
        &self,
        object: *const ObjectHeader,
        object_address: usize,
        name: &[u8],
    ) -> OwnField {
        let keys = (*object).keys_array;
        if keys.is_null() {
            return OwnField::Missing;
        }
        let Some(keys_header) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
            return OwnField::Invalid;
        };
        if keys_header.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return OwnField::Invalid;
        }
        let key_len = (*keys).length as usize;
        let key_capacity = (*keys).capacity as usize;
        let required = match crate::gc::GC_HEADER_SIZE
            .checked_add(std::mem::size_of::<ArrayHeader>())
            .and_then(|size| {
                key_capacity
                    .checked_mul(std::mem::size_of::<f64>())
                    .and_then(|slots| size.checked_add(slots))
            }) {
            Some(required) => required,
            None => return OwnField::Invalid,
        };
        if key_len > key_capacity
            || key_len > MAX_CONTAINER_LEN
            || required > keys_header.size as usize
        {
            return OwnField::Invalid;
        }
        let key_slots = (keys as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for index in 0..key_len {
            let key = JSValue::from_bits(std::ptr::read(key_slots.add(index)).to_bits());
            if crate::string::js_string_key_matches_bytes(key, name) {
                let Ok(name) = std::str::from_utf8(name) else {
                    return OwnField::Invalid;
                };
                if crate::object::get_accessor_descriptor(object_address, name).is_some() {
                    return OwnField::Invalid;
                }
                return OwnField::Data(crate::object::js_object_get_field(object, index as u32));
            }
        }
        OwnField::Missing
    }

    unsafe fn matches(&mut self, value: JSValue, node_id: u32, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let Some(node) = self.descriptor.node(node_id) else {
            return false;
        };
        let Some(op) = node.first().copied() else {
            return false;
        };
        match op {
            OP_ANY => node.len() == 1,
            OP_NUMBER => node.len() == 1 && (value.is_number() || value.is_int32()),
            OP_INT32 => {
                node.len() == 1
                    && crate::native_abi::js_typed_i32_arg_guard(f64::from_bits(value.bits())) != 0
            }
            OP_BOOLEAN => node.len() == 1 && matches!(value.bits(), TAG_TRUE | TAG_FALSE),
            OP_STRING => node.len() == 1 && value.is_any_string(),
            OP_NULL => node.len() == 1 && value.is_null(),
            OP_UNDEFINED => node.len() == 1 && value.is_undefined(),
            OP_BIGINT => node.len() == 1 && value.is_bigint(),
            OP_SYMBOL => {
                node.len() == 1
                    && value.is_pointer()
                    && crate::symbol::is_registered_symbol((value.bits() & POINTER_MASK) as usize)
            }
            OP_STRING_LITERAL => {
                let Some(length) = read_u32(node, 1).map(|value| value as usize) else {
                    return false;
                };
                node.len() == 5 + length
                    && crate::string::js_string_key_matches_bytes(value, &node[5..])
            }
            OP_ARRAY => {
                let Some(child) = read_u32(node, 1) else {
                    return false;
                };
                if node.len() != 5 || self.descriptor.node(child).is_none() {
                    return false;
                }
                let Some((array, length)) = self.plain_array(value) else {
                    return false;
                };
                if self.seen_or_insert(array as usize, node_id) {
                    return true;
                }
                let elements =
                    (array as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
                for index in 0..length {
                    let element = JSValue::from_bits(std::ptr::read(elements.add(index)).to_bits());
                    if element.bits() == TAG_HOLE || !self.matches(element, child, depth + 1) {
                        return false;
                    }
                }
                true
            }
            OP_TUPLE => {
                let Some(count) = read_u32(node, 1).map(|value| value as usize) else {
                    return false;
                };
                if node.len() != 5usize.saturating_add(count.saturating_mul(4)) {
                    return false;
                }
                let Some((array, length)) = self.plain_array(value) else {
                    return false;
                };
                if length != count {
                    return false;
                }
                if self.seen_or_insert(array as usize, node_id) {
                    return true;
                }
                let elements =
                    (array as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
                for index in 0..count {
                    let Some(child) = read_u32(node, 5 + index * 4) else {
                        return false;
                    };
                    let element = JSValue::from_bits(std::ptr::read(elements.add(index)).to_bits());
                    if element.bits() == TAG_HOLE || !self.matches(element, child, depth + 1) {
                        return false;
                    }
                }
                true
            }
            OP_OBJECT => {
                let (Some(class_id), Some(field_count)) = (
                    read_u32(node, 1),
                    read_u32(node, 5).map(|value| value as usize),
                ) else {
                    return false;
                };
                let Some((object, address)) = self.plain_object(value) else {
                    return false;
                };
                if class_id != 0
                    && !crate::object::class_chain_reaches((*object).class_id, class_id)
                {
                    return false;
                }
                if self.seen_or_insert(address, node_id) {
                    return true;
                }
                let mut cursor = 9usize;
                let mut valid = true;
                for _ in 0..field_count {
                    let Some(optional) = node.get(cursor).copied() else {
                        valid = false;
                        break;
                    };
                    let Some(name_len) = read_u16(node, cursor + 1).map(|value| value as usize)
                    else {
                        valid = false;
                        break;
                    };
                    let name_start = cursor + 3;
                    let name_end = name_start.saturating_add(name_len);
                    let Some(name) = node.get(name_start..name_end) else {
                        valid = false;
                        break;
                    };
                    let Some(child) = read_u32(node, name_end) else {
                        valid = false;
                        break;
                    };
                    cursor = name_end + 4;
                    match self.own_data_field(object, address, name) {
                        OwnField::Data(field) if optional != 0 && field.is_undefined() => {}
                        OwnField::Data(field) if self.matches(field, child, depth + 1) => {}
                        OwnField::Missing if optional != 0 => {}
                        _ => {
                            valid = false;
                            break;
                        }
                    }
                }
                valid && cursor == node.len()
            }
            OP_UNION => {
                let Some(count) = read_u32(node, 1).map(|value| value as usize) else {
                    return false;
                };
                if count == 0 || node.len() != 5usize.saturating_add(count.saturating_mul(4)) {
                    return false;
                }
                for index in 0..count {
                    let Some(child) = read_u32(node, 5 + index * 4) else {
                        return false;
                    };
                    let checkpoint = self.checkpoint();
                    if self.matches(value, child, depth + 1) {
                        return true;
                    }
                    self.rollback(checkpoint);
                }
                false
            }
            OP_RECURSIVE_REF => read_u32(node, 1)
                .is_some_and(|target| node.len() == 5 && self.matches(value, target, depth + 1)),
            OP_MAP => {
                let (Some(key_node), Some(value_node)) = (read_u32(node, 1), read_u32(node, 5))
                else {
                    return false;
                };
                if node.len() != 9
                    || self.descriptor.node(key_node).is_none()
                    || self.descriptor.node(value_node).is_none()
                {
                    return false;
                }
                let Some((map, size)) = self.plain_map(value) else {
                    return false;
                };
                if self.seen_or_insert(map as usize, node_id) {
                    return true;
                }
                let entries = (*map).entries as *const f64;
                for index in 0..size {
                    let key = JSValue::from_bits(std::ptr::read(entries.add(index * 2)).to_bits());
                    let value =
                        JSValue::from_bits(std::ptr::read(entries.add(index * 2 + 1)).to_bits());
                    if !self.matches(key, key_node, depth + 1)
                        || !self.matches(value, value_node, depth + 1)
                    {
                        return false;
                    }
                }
                true
            }
            OP_SET => {
                let Some(child) = read_u32(node, 1) else {
                    return false;
                };
                if node.len() != 5 || self.descriptor.node(child).is_none() {
                    return false;
                }
                let Some((set, size)) = self.plain_set(value) else {
                    return false;
                };
                if self.seen_or_insert(set as usize, node_id) {
                    return true;
                }
                let elements = (*set).elements as *const f64;
                for index in 0..size {
                    let element = JSValue::from_bits(std::ptr::read(elements.add(index)).to_bits());
                    if !self.matches(element, child, depth + 1) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }
}

/// Return 1 only when `value` satisfies the complete compiler descriptor.
/// Invalid descriptors and values conservatively return 0.
#[no_mangle]
pub extern "C" fn js_param_type_guard(value: f64, descriptor: *const u8, length: u32) -> i32 {
    let length = length as usize;
    if descriptor.is_null() || length == 0 || length > MAX_DESCRIPTOR_LEN {
        return 0;
    }
    // SAFETY: generated code passes a pointer to an immutable constant whose
    // allocation is exactly `length` bytes. The length cap prevents hostile
    // descriptors from manufacturing an unbounded slice.
    let bytes = unsafe { std::slice::from_raw_parts(descriptor, length) };
    let Some(descriptor) = Descriptor::parse(bytes) else {
        return 0;
    };
    let root = descriptor.root;
    let mut state = GuardState {
        descriptor,
        inline_visited: [(0, 0); INLINE_VISITED],
        inline_visited_len: 0,
        spill_visited: None,
        spill_log: Vec::new(),
    };
    unsafe { state.matches(JSValue::from_bits(value.to_bits()), root, 0) as i32 }
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PARAM_TYPE_GUARD: extern "C" fn(f64, *const u8, u32) -> i32 = js_param_type_guard;

#[cfg(test)]
mod tests {
    use super::*;

    fn one_node(body: &[u8]) -> Vec<u8> {
        descriptor(0, &[body])
    }

    fn descriptor(root: u32, bodies: &[&[u8]]) -> Vec<u8> {
        let start = 12 + (bodies.len() as u32 + 1) * 4;
        let mut descriptor = Vec::new();
        descriptor.extend_from_slice(&MAGIC.to_le_bytes());
        descriptor.extend_from_slice(&root.to_le_bytes());
        descriptor.extend_from_slice(&(bodies.len() as u32).to_le_bytes());
        let mut offset = start;
        descriptor.extend_from_slice(&offset.to_le_bytes());
        for body in bodies {
            offset += body.len() as u32;
            descriptor.extend_from_slice(&offset.to_le_bytes());
        }
        for body in bodies {
            descriptor.extend_from_slice(body);
        }
        descriptor
    }

    fn guard(value: JSValue, descriptor: &[u8]) -> i32 {
        js_param_type_guard(
            f64::from_bits(value.bits()),
            descriptor.as_ptr(),
            descriptor.len() as u32,
        )
    }

    #[test]
    fn primitive_descriptors_reject_lying_values() {
        let number = one_node(&[OP_NUMBER]);
        assert_eq!(guard(JSValue::number(12.5), &number), 1);
        assert_eq!(guard(JSValue::int32(12), &number), 1);
        assert_eq!(guard(JSValue::bool(true), &number), 0);

        let boolean = one_node(&[OP_BOOLEAN]);
        assert_eq!(guard(JSValue::bool(false), &boolean), 1);
        assert_eq!(guard(JSValue::number(1.0), &boolean), 0);
    }

    #[test]
    fn malformed_descriptors_fail_closed() {
        assert_eq!(js_param_type_guard(1.0, std::ptr::null(), 0), 0);
        let mut descriptor = one_node(&[OP_NUMBER]);
        descriptor[0] = 0;
        assert_eq!(guard(JSValue::number(1.0), &descriptor), 0);
    }

    #[test]
    fn collection_descriptors_validate_every_entry() {
        let map_descriptor = descriptor(
            0,
            &[
                &[OP_MAP, 1, 0, 0, 0, 2, 0, 0, 0],
                &[OP_STRING],
                &[OP_NUMBER],
            ],
        );
        let map = crate::map::js_map_alloc(2);
        let key = crate::string::js_string_from_bytes(b"rate".as_ptr(), 4);
        crate::map::js_map_set(map, crate::value::js_nanbox_string(key as i64), 3.0);
        let map_value = JSValue::from_bits(crate::value::js_nanbox_pointer(map as i64).to_bits());
        assert_eq!(guard(map_value, &map_descriptor), 1);
        crate::map::js_map_set(
            map,
            crate::value::js_nanbox_string(key as i64),
            crate::value::js_nanbox_string(key as i64),
        );
        assert_eq!(guard(map_value, &map_descriptor), 0);

        let set_descriptor = descriptor(0, &[&[OP_SET, 1, 0, 0, 0], &[OP_BOOLEAN]]);
        let set = crate::set::js_set_alloc(2);
        crate::set::js_set_add(set, f64::from_bits(TAG_TRUE));
        let set_value = JSValue::from_bits(crate::value::js_nanbox_pointer(set as i64).to_bits());
        assert_eq!(guard(set_value, &set_descriptor), 1);
        crate::set::js_set_add(set, 7.0);
        assert_eq!(guard(set_value, &set_descriptor), 0);
    }
}
