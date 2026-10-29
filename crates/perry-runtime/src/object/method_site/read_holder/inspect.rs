//! `Get(O, key)` inspected from shape facts without running anything: the
//! receiver's chain walked by key-list position, hop by hop. Nothing is
//! remembered; a builtin asks again on every call. The unknown-receiver probe
//! answers most receivers first; this walk also admits the builtin
//! prototypes the probe declines (a RegExp's `exec`, its flag getters).
use super::*;
use crate::object::field_get_set::runtime_read_site::{Answer, Key};

unsafe fn position(obj: *const ObjectHeader, key: Key<'_>) -> Option<u32> {
    let keys = crate::object::object_keys(obj);
    match key {
        Key::Name(name) => {
            crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), name)
        }
        Key::Symbol(symbol) => crate::object::shaped_symbols::position(obj, symbol),
    }
}

/// The accessor pair a lane names, when it is one.
unsafe fn valid_pair(lane: u64) -> Option<usize> {
    if lane & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let pair = (lane & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(pair)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || (*(pair as *const crate::array::ArrayHeader)).length < 2
    {
        return None;
    }
    Some(pair)
}

/// What `Get(obj, key)` returns or calls, when the shapes of the receiver and
/// of at most [`HOLDER_MAX_DEPTH`] prototypes prove it; `None` otherwise
/// (including a proven absence: a builtin prototype's list does not prove
/// one).
///
/// # Safety
/// `obj` is a live object above the handle band.
pub(crate) unsafe fn shape_answer(obj: *const ObjectHeader, key: Key<'_>) -> Option<Answer> {
    if crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_depth() != 0
    {
        return None;
    }
    let recv = super::super::ordinary_receiver(obj as usize)?;
    if let Key::Name(name) = key {
        if name == b"constructor" {
            if !admitted_proto_id(recv).is_some_and(|id| id < PROTO_ID_CLASS) {
                return None;
            }
        } else if !holder_name_admitted(name) {
            return None;
        }
    }
    let mut current = recv;
    for depth in 0..=HOLDER_MAX_DEPTH {
        let current_obj = super::super::ordinary_receiver(current as usize)?;
        let shape = object_shape_descriptor(current_obj)?;
        if let Some(index) = position(current_obj, key) {
            let keys = shape.keys as usize as *const crate::array::ArrayHeader;
            let attrs = crate::object::key_attrs::keys_entry(keys, index);
            if attrs & crate::object::key_attrs::ENTRY_PRIVATE != 0 {
                return None;
            }
            let slot = holder_slot_word(
                current_obj as usize,
                keys,
                index,
                shape.live_inline_slot_count,
            )?;
            let lane = holder_slot_value(current_obj as usize, slot)?;
            if attrs & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
                let pair = valid_pair(lane)?;
                let slots =
                    crate::array::array_elements_ptr(pair as *const crate::array::ArrayHeader);
                return Some(Answer::Getter(
                    *slots.add(crate::object::accessor_pair::PAIR_GET),
                ));
            }
            return (lane != crate::value::TAG_HOLE).then_some(Answer::Data(lane));
        }
        if depth == HOLDER_MAX_DEPTH {
            return None;
        }
        current = accessor_holder(current_obj)?.0;
    }
    None
}

/// The object `recv`'s ShapeId pins as its direct prototype, for an accessor
/// entry: a declared class's prototype ([`class_link`]; `true`), or the object
/// a serial or default prototype identity names (`false`).
unsafe fn accessor_holder(recv: *const ObjectHeader) -> Option<(*const ObjectHeader, bool)> {
    if let Some(holder) = class_link(recv) {
        return Some((holder, true));
    }
    let (pid, word) = admitted_link(recv)?;
    let holder = match pid {
        PROTO_ID_NULL => return None,
        PROTO_ID_DEFAULT => {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        }
        _ => next_from_word(recv, word),
    };
    (!holder.is_null() && holder != recv).then_some((holder, false))
}
