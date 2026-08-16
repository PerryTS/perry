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
use std::mem::MaybeUninit;

const MAGIC: u32 = 0x3254_4750; // `PGT2`, little-endian.

/// Set by the compiler on a container node's op byte when a visit to that
/// node MUST be recorded in the traversal's visited set (#8202).
///
/// The set exists for two reasons, and only nodes that can hit either one
/// need to pay for it:
///
/// * **termination** — a value cycle (`env.parent === env`) can only walk
///   forever through a descriptor node that reaches itself, so a node off
///   every descriptor cycle cannot loop;
/// * **no re-walk blowup** — a node the traversal can enter twice with the
///   same address memoizes, which keeps total work linear in (address, node)
///   pairs. Below that, every node is entered once per address and the memo
///   could never hit.
///
/// `visit_tracking_bits` in `perry-codegen`'s `codegen::param_guard` decides
/// both from the immutable graph it just built, so the runtime reads the
/// answer instead of recomputing it per call. A tree-shaped
/// descriptor — `{ toks: Token[], pos: number }`, a union of object literals —
/// now records nothing at all, where before every container visit paid a
/// linear scan of up to 64 entries and then a `HashSet` insert per element.
const OP_TRACK_VISIT: u8 = 0x80;
const OP_MASK: u8 = 0x7F;
const MAX_DESCRIPTOR_LEN: usize = 1 << 20;
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 256;
/// Cumulative node visits allowed in one validation. `MAX_DEPTH` bounds how
/// DEEP the walk goes, not how much of it runs. #8238 stopped recording a
/// visit for descriptor nodes that are neither on a cycle nor reachable two
/// ways, which is exactly right for the descriptor graph — but a *value* can
/// still re-enter such a node with the same address, by holding one object at
/// many indices of an array. Nesting that duplication multiplies, so an
/// untracked subtree that used to be memoized into one walk can run k^d times.
/// Exhausting the budget fails the guard, which is the same safe direction as
/// the depth cap: the caller falls back to the generic function. A million
/// checks is also the point past which the guard has lost on its own terms —
/// no specialized call recoups that — so the fallback is the better choice
/// here even when the walk would have terminated.
const MAX_VISITS: u32 = 1 << 20;
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
    /// First byte past the offset table, validated once by `parse`. `node`
    /// used to recompute it from `node_count` on every single node visit.
    table_end: usize,
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
            table_end,
        })
    }

    fn node(&self, id: u32) -> Option<&'a [u8]> {
        let index = id as usize;
        if index >= self.node_count {
            return None;
        }
        let start = read_u32(self.bytes, 12 + index * 4)? as usize;
        let end = read_u32(self.bytes, 12 + (index + 1) * 4)? as usize;
        if start < self.table_end || end < start {
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
    ///
    /// Deliberately uninitialised (#8202): only `[..inline_visited_len]` is
    /// ever read, and most guarded calls insert nothing at all, so zeroing
    /// 1 KB of stack on entry was pure fixed cost on every guarded call.
    inline_visited: [MaybeUninit<(usize, u32)>; INLINE_VISITED],
    inline_visited_len: usize,
    spill_visited: Option<HashSet<(usize, u32)>>,
    spill_log: Vec<(usize, u32)>,
    /// Cumulative `matches` entries, capped by `MAX_VISITS`.
    visits: u32,
    /// The last object `plain_object` validated, keyed on the NaN-box bits it
    /// came from (#8202). A union tries its arms against the SAME value, so
    /// every arm past the first re-ran the whole validation — including the
    /// shape-table probe, which is the object model's hottest lookup. Nothing
    /// between two arms can invalidate it: validation runs no JavaScript and
    /// allocates nothing, so no collection can move or mutate the object.
    validated_object: Option<(u64, ValidObject)>,
}

/// A `plain_object` result: the header, its address, and the live inline-slot
/// bound its ShapeId descriptor publishes.
#[derive(Clone, Copy)]
struct ValidObject {
    object: *const ObjectHeader,
    address: usize,
    live_slots: usize,
}

enum OwnField {
    Missing,
    Data(JSValue),
    Invalid,
}

/// A validated `keys_array`, resolved once per object rather than per field.
enum ObjectKeys {
    /// No keys array at all: every field reads as `Missing`.
    Absent,
    /// The keys array failed header validation: every field is `Invalid`.
    Invalid,
    Present {
        slots: *const f64,
        len: usize,
    },
}

impl GuardState<'_> {
    fn checkpoint(&self) -> usize {
        self.inline_visited_len + self.spill_log.len()
    }

    fn seen_or_insert(&mut self, address: usize, node_id: u32) -> bool {
        let key = (address, node_id);
        // SAFETY: every slot below `inline_visited_len` was written by an
        // earlier insert. `rollback` only lowers the length, so a slot can go
        // back out of range but never becomes readable while uninitialised.
        let seen_inline = self.inline_visited[..self.inline_visited_len]
            .iter()
            .any(|slot| unsafe { slot.assume_init() } == key);
        if seen_inline
            || self
                .spill_visited
                .as_ref()
                .is_some_and(|visited| visited.contains(&key))
        {
            return true;
        }
        if self.inline_visited_len < INLINE_VISITED {
            self.inline_visited[self.inline_visited_len] = MaybeUninit::new(key);
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

    /// A validated ordinary object: its header pointer, its address, and the
    /// live inline-slot bound its ShapeId descriptor publishes (#8113/#8122 —
    /// the one fact `own_data_field` needs to read a slot without probing the
    /// shape table again per field).
    unsafe fn plain_object(
        &mut self,
        value: JSValue,
    ) -> Option<(*const ObjectHeader, usize, usize)> {
        if !value.is_pointer() {
            return None;
        }
        if let Some((bits, cached)) = self.validated_object {
            if bits == value.bits() {
                return Some((cached.object, cached.address, cached.live_slots));
            }
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
        // both the receiver kind and the live inline-slot bound come from the
        // ShapeId descriptor. ONE probe: this runs on every guarded call, and
        // `object_is_regular` + `object_live_slot_count` would be two probes
        // plus a second read of the GcHeader already validated above
        // (measured +3% instructions on `interp`/`iso_miss`).
        let Some(descriptor) = crate::object::shapes::object_shape_descriptor(object) else {
            return None;
        };
        if descriptor.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary {
            return None;
        }
        let live_slots = descriptor.live_inline_slot_count as usize;
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
        self.validated_object = Some((
            value.bits(),
            ValidObject {
                object,
                address,
                live_slots,
            },
        ));
        Some((object, address, live_slots))
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

    /// Resolve and validate the object's `keys_array` ONCE per object (#8202).
    /// This ran per descriptor FIELD, so a two-field object paid for the whole
    /// header validation twice.
    unsafe fn object_keys(&self, object: *const ObjectHeader) -> ObjectKeys {
        let keys = (*object).keys_array;
        if keys.is_null() {
            return ObjectKeys::Absent;
        }
        let Some(keys_header) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
            return ObjectKeys::Invalid;
        };
        if keys_header.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return ObjectKeys::Invalid;
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
            None => return ObjectKeys::Invalid,
        };
        if key_len > key_capacity
            || key_len > MAX_CONTAINER_LEN
            || required > keys_header.size as usize
        {
            return ObjectKeys::Invalid;
        }
        ObjectKeys::Present {
            slots: (keys as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64,
            len: key_len,
        }
    }

    unsafe fn own_data_field(
        &self,
        object: *const ObjectHeader,
        object_address: usize,
        live_slots: usize,
        keys: &ObjectKeys,
        may_have_accessors: bool,
        name: &[u8],
    ) -> OwnField {
        let (key_slots, key_len) = match *keys {
            ObjectKeys::Absent => return OwnField::Missing,
            ObjectKeys::Invalid => return OwnField::Invalid,
            ObjectKeys::Present { slots, len } => (slots, len),
        };
        for index in 0..key_len {
            let key = JSValue::from_bits(std::ptr::read(key_slots.add(index)).to_bits());
            if crate::string::js_string_key_matches_bytes(key, name) {
                // #8202: `get_accessor_descriptor` ran per field per guarded
                // call, and its own per-key prefilter still cost a UTF-8
                // validation plus a key hash. `owner_may_have_descriptor_entries`
                // answers "does this object own ANY accessor at all" from one
                // meta word, which is `false` for every ordinary object, so the
                // per-field work disappears for the whole common case.
                if may_have_accessors {
                    let Ok(name) = std::str::from_utf8(name) else {
                        return OwnField::Invalid;
                    };
                    if crate::object::get_accessor_descriptor(object_address, name).is_some() {
                        return OwnField::Invalid;
                    }
                }
                // #8122: read the inline slot directly against the bound
                // `plain_object` already resolved from the descriptor. Going
                // through `js_object_get_field` re-derived that bound with a
                // shape-table probe per field per guarded call — measured +3%
                // instructions on `interp`/`iso_miss` after #8113 replaced the
                // header's `field_count` word with the descriptor. Slots past
                // the inline bound (spill) still take the runtime getter,
                // which owns that path.
                if index < live_slots {
                    let fields = (object as *const u8).add(std::mem::size_of::<ObjectHeader>())
                        as *const u64;
                    let bits = std::ptr::read(fields.add(index));
                    // Mirror `js_object_get_field`: a null POINTER_TAG payload
                    // is never a legitimate value and reads as `undefined`.
                    if bits == 0x7FFD_0000_0000_0000 {
                        return OwnField::Data(JSValue::undefined());
                    }
                    return OwnField::Data(JSValue::from_bits(bits));
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
        self.visits += 1;
        if self.visits > MAX_VISITS {
            return false;
        }
        let Some(node) = self.descriptor.node(node_id) else {
            return false;
        };
        let Some(tagged_op) = node.first().copied() else {
            return false;
        };
        // The compiler folds the visit-tracking decision into the op byte's
        // high bit; the low seven bits are the op itself (#8202).
        let track = tagged_op & OP_TRACK_VISIT != 0;
        let op = tagged_op & OP_MASK;
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
                if track && self.seen_or_insert(array as usize, node_id) {
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
                if track && self.seen_or_insert(array as usize, node_id) {
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
                let Some((object, address, live_slots)) = self.plain_object(value) else {
                    return false;
                };
                if class_id != 0
                    && !crate::object::class_chain_reaches((*object).class_id, class_id)
                {
                    return false;
                }
                if track && self.seen_or_insert(address, node_id) {
                    return true;
                }
                let (keys, may_have_accessors) = if field_count == 0 {
                    (ObjectKeys::Absent, false)
                } else {
                    (
                        self.object_keys(object),
                        crate::object::owner_may_have_descriptor_entries(address, true),
                    )
                };
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
                    match self.own_data_field(
                        object,
                        address,
                        live_slots,
                        &keys,
                        may_have_accessors,
                        name,
                    ) {
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
                if track && self.seen_or_insert(map as usize, node_id) {
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
                if track && self.seen_or_insert(set as usize, node_id) {
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
        inline_visited: [const { MaybeUninit::uninit() }; INLINE_VISITED],
        inline_visited_len: 0,
        spill_visited: None,
        spill_log: Vec::new(),
        visits: 0,
        validated_object: None,
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

    fn object_node(class_id: u32, fields: &[(bool, &[u8], u32)]) -> Vec<u8> {
        let mut body = vec![OP_OBJECT];
        body.extend_from_slice(&class_id.to_le_bytes());
        body.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        for (optional, name, child) in fields {
            body.push(u8::from(*optional));
            body.extend_from_slice(&(name.len() as u16).to_le_bytes());
            body.extend_from_slice(name);
            body.extend_from_slice(&child.to_le_bytes());
        }
        body
    }

    /// `{ kind: "num", num: <num> }` as a real heap object.
    fn num_node_object(num: f64) -> (*mut ObjectHeader, JSValue) {
        let object = crate::object::js_object_alloc(0, 0);
        let kind_key = crate::string::js_string_from_bytes(b"kind".as_ptr(), 4);
        let kind = crate::string::js_string_from_bytes(b"num".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            object,
            kind_key,
            crate::value::js_nanbox_string(kind as i64),
        );
        let num_key = crate::string::js_string_from_bytes(b"num".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(object, num_key, num);
        let value = JSValue::from_bits(crate::value::js_nanbox_pointer(object as i64).to_bits());
        (object, value)
    }

    /// `{ kind: "num"; num: number } | { kind: "str"; str: string }` — the
    /// two-arm shape whose arms both re-validate the SAME object.
    fn value_union() -> Vec<u8> {
        descriptor(
            4,
            &[
                &[OP_STRING_LITERAL, 3, 0, 0, 0, b'n', b'u', b'm'],
                &[OP_NUMBER],
                &[OP_STRING_LITERAL, 3, 0, 0, 0, b's', b't', b'r'],
                &[OP_STRING],
                &[OP_UNION, 2, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0],
                &object_node(0, &[(false, b"kind", 0), (false, b"num", 1)]),
                &object_node(0, &[(false, b"kind", 2), (false, b"str", 3)]),
            ],
        )
    }

    /// The per-call `plain_object` reuse must not change any arm's verdict:
    /// a union retries every arm against the SAME value, and only the arm
    /// whose literal and field types match may accept it (#8202).
    #[test]
    fn union_arms_retried_against_one_object_still_decide_per_arm() {
        let _global = crate::gc::global_side_table_test_lock();
        let union = value_union();
        let (_, num_value) = num_node_object(7.0);
        assert_eq!(guard(num_value, &union), 1);

        // Same shape, but `num` holds a string: arm 0's literal matches and
        // its field type does not, arm 1's literal does not match. Reject.
        let liar = crate::object::js_object_alloc(0, 0);
        let kind_key = crate::string::js_string_from_bytes(b"kind".as_ptr(), 4);
        let kind = crate::string::js_string_from_bytes(b"num".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            liar,
            kind_key,
            crate::value::js_nanbox_string(kind as i64),
        );
        let num_key = crate::string::js_string_from_bytes(b"num".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            liar,
            num_key,
            crate::value::js_nanbox_string(kind as i64),
        );
        let liar = JSValue::from_bits(crate::value::js_nanbox_pointer(liar as i64).to_bits());
        assert_eq!(guard(liar, &union), 0);
    }

    /// An accessor shadowing a descriptor field must still be refused —
    /// reading it would run user code. #8202 replaced the per-field probe
    /// with a per-object summary, so this is what proves the summary is not
    /// simply always-false.
    #[test]
    fn an_accessor_shadowing_a_field_is_still_refused() {
        let _global = crate::gc::global_side_table_test_lock();
        let union = value_union();
        let (object, value) = num_node_object(7.0);
        assert_eq!(guard(value, &union), 1, "plain data object validates");

        crate::object::set_accessor_descriptor(
            object as usize,
            "num".to_string(),
            crate::object::AccessorDescriptor { get: 1, set: 0 },
        );
        assert_eq!(
            guard(value, &union),
            0,
            "an accessor over `num` must take the generic fallback"
        );
        crate::object::clear_accessor_descriptor(object as usize, "num");
        assert_eq!(guard(value, &union), 1, "and validate again once cleared");
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

    /// `{ <name>: <child> }`, optionally carrying the compiler's
    /// visit-tracking bit, as one object node.
    fn tracked_single_field_node(track: bool, name: &[u8], child: u32) -> Vec<u8> {
        let mut body = vec![if track {
            OP_OBJECT | OP_TRACK_VISIT
        } else {
            OP_OBJECT
        }];
        body.extend_from_slice(&0u32.to_le_bytes()); // class_id: structural
        body.extend_from_slice(&1u32.to_le_bytes()); // one field
        body.push(0); // required
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(name);
        body.extend_from_slice(&child.to_le_bytes());
        body
    }

    fn plain_object(fields: &[(&[u8], JSValue)]) -> (*mut ObjectHeader, JSValue) {
        let object = crate::object::js_object_alloc(0, fields.len() as u32);
        for (name, value) in fields {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(object, key, f64::from_bits(value.bits()));
        }
        let value = JSValue::from_bits(crate::value::js_nanbox_pointer(object as i64).to_bits());
        (object, value)
    }

    /// (#8202) The bit is the validator's ONLY termination argument for a
    /// value cycle: `node.next === node` against a self-referential
    /// descriptor node must memoize on the second arrival and accept.
    #[test]
    fn a_tracked_node_terminates_on_a_cyclic_value() {
        let (_, node) = plain_object(&[(b"next", JSValue::undefined())]);
        let key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
        crate::object::js_object_set_field_by_name(
            crate::value::js_nanbox_get_pointer(f64::from_bits(node.bits())) as *mut ObjectHeader,
            key,
            f64::from_bits(node.bits()),
        );

        assert_eq!(
            guard(node, &descriptor(0, &[&tracked_single_field_node(true, b"next", 0)])),
            1
        );
        // Without it the walk runs out of depth and the caller conservatively
        // takes the generic function — never a hang, never a false accept.
        assert_eq!(
            guard(node, &descriptor(0, &[&tracked_single_field_node(false, b"next", 0)])),
            0
        );
    }

    /// An untracked node is a pure cost saving, not a semantic change: a
    /// shared address reached twice re-walks and still decides the same way.
    #[test]
    fn an_untracked_shared_node_decides_the_same_way() {
        let (_, leaf) = plain_object(&[(b"v", JSValue::number(1.0))]);
        let (_, pair) = plain_object(&[(b"a", leaf), (b"b", leaf)]);

        let mut pair_body = vec![OP_OBJECT];
        pair_body.extend_from_slice(&0u32.to_le_bytes());
        pair_body.extend_from_slice(&2u32.to_le_bytes());
        for name in [b"a", b"b"] {
            pair_body.push(0);
            pair_body.extend_from_slice(&1u16.to_le_bytes());
            pair_body.extend_from_slice(name);
            pair_body.extend_from_slice(&1u32.to_le_bytes());
        }
        let leaf_body = tracked_single_field_node(false, b"v", 2);
        let blob = descriptor(0, &[&pair_body, &leaf_body, &[OP_NUMBER]]);
        assert_eq!(guard(pair, &blob), 1);

        let mut mismatched = blob.clone();
        *mismatched.last_mut().unwrap() = OP_STRING;
        assert_eq!(guard(pair, &mismatched), 0);
    }

    /// #8238 drops the visit record for nodes that are neither on a cycle nor
    /// reachable two ways in the DESCRIPTOR graph. A *value* can still re-enter
    /// such a node with the same address, by holding one object at several
    /// fields, and nesting that duplication multiplies: `d` levels of a
    /// two-way share re-walk the leaf 2^d times where the memoized walk ran it
    /// once. `MAX_DEPTH` does not bound that — it bounds depth, not total work.
    /// `MAX_VISITS` does, in the same safe direction as the depth cap.
    #[test]
    fn nested_value_duplication_through_untracked_nodes_is_bounded() {
        const LEVELS: u32 = 40;

        // Descriptor: LEVELS untracked `{a: next, b: next}` nodes over a number.
        // Every node is single-entry and acyclic, so #8238 leaves them all
        // untracked — this is precisely the shape the analysis declines to mark.
        let mut nodes: Vec<Vec<u8>> = Vec::new();
        for level in 0..LEVELS {
            let mut body = vec![OP_OBJECT];
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&2u32.to_le_bytes());
            for name in [b"a", b"b"] {
                body.push(0);
                body.extend_from_slice(&1u16.to_le_bytes());
                body.extend_from_slice(name);
                body.extend_from_slice(&(level + 1).to_le_bytes());
            }
            nodes.push(body);
        }
        nodes.push(vec![OP_NUMBER]);
        let refs: Vec<&[u8]> = nodes.iter().map(|n| n.as_slice()).collect();
        let blob = descriptor(0, &refs);

        // Value: the same child at BOTH fields, all the way down.
        let mut value = JSValue::number(1.0);
        for _ in 0..LEVELS {
            value = plain_object(&[(b"a", value), (b"b", value)]).1;
        }

        // Unbounded this is 2^40 visits. The budget stops it and fails the
        // guard, so the caller takes the generic function.
        assert_eq!(guard(value, &blob), 0);
    }

    /// The tracking bits changed what the op byte means, so a descriptor from
    /// a compiler that predates them must not be read as one that opts out of
    /// tracking everywhere — it fails closed on the magic instead.
    #[test]
    fn the_previous_descriptor_format_is_refused() {
        let mut previous = one_node(&[OP_NUMBER]);
        previous[0..4].copy_from_slice(&0x3154_4750u32.to_le_bytes());
        assert_eq!(guard(JSValue::number(1.0), &previous), 0);
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
