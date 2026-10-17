//! Births of node's emitter and stream instances on their static layouts
//! (`runtime_layouts::stream_instances`, object model §3.2 and §3.6).
//!
//! A direct `new X(opts)` whose live `X.prototype` is the sealed prototype
//! its layout names is born on that layout: one compare of the prototype
//! with its family's identity word, and one compare per prototype of the chain
//! of its layout word with the end of the unshadowed range (each is sealed,
//! so every write since sealing set its shadow bit). Such a chain holds exactly the
//! runtime's definitions, none an accessor or read-only data for an instance
//! key, so the constructor's `[[Set]]`s are own-property creations and fill
//! the slots. Any other prototype takes the ordinary construction.
use crate::object::layout_desc::{PerryLayoutDesc, LAYOUT_PROTO_NULL, LAYOUT_PROTO_OBJECT};
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// The declared layout of the sealed node prototype of intrinsic `serial`.
fn prototype_layout(serial: u64) -> Option<*const PerryLayoutDesc> {
    use crate::closure::shape::*;
    use crate::object::runtime_layouts as rl;
    Some(match serial {
        INTRINSIC_SERIAL_EVENT_EMITTER_PROTOTYPE => &raw const rl::EVENT_EMITTER_PROTOTYPE,
        INTRINSIC_SERIAL_STREAM_PROTOTYPE => &raw const rl::STREAM_PROTOTYPE,
        INTRINSIC_SERIAL_READABLE_PROTOTYPE => &raw const rl::READABLE_PROTOTYPE,
        INTRINSIC_SERIAL_WRITABLE_PROTOTYPE => &raw const rl::WRITABLE_PROTOTYPE,
        INTRINSIC_SERIAL_DUPLEX_PROTOTYPE => &raw const rl::DUPLEX_PROTOTYPE,
        INTRINSIC_SERIAL_TRANSFORM_PROTOTYPE => &raw const rl::TRANSFORM_PROTOTYPE,
        INTRINSIC_SERIAL_PASSTHROUGH_PROTOTYPE => &raw const rl::PASSTHROUGH_PROTOTYPE,
        INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE => &raw const rl::OUTGOING_MESSAGE_PROTOTYPE,
        INTRINSIC_SERIAL_SERVER_RESPONSE_PROTOTYPE => &raw const rl::SERVER_RESPONSE_PROTOTYPE,
        INTRINSIC_SERIAL_INCOMING_MESSAGE_PROTOTYPE => &raw const rl::INCOMING_MESSAGE_PROTOTYPE,
        _ => return None,
    })
}

/// Is `proto` the sealed prototype `layout` names, with every prototype of
/// its chain unwritten since sealing?
///
/// # Safety
/// `layout` is a registered instance layout naming a node prototype family.
pub(crate) unsafe fn sealed_chain(proto: f64, layout: *const PerryLayoutDesc) -> bool {
    let mut family = (*layout).proto;
    let mut first = true;
    loop {
        if family == LAYOUT_PROTO_OBJECT {
            return crate::object::builtin_proto::pristine(
                crate::object::builtin_proto::BuiltinProto::Object,
            );
        }
        if family == LAYOUT_PROTO_NULL {
            return true;
        }
        let serial = (*family).serial;
        let bits = crate::object::shapes::identity_prototype_word(serial);
        if first && bits != proto.to_bits() {
            return false;
        }
        first = false;
        let Some(desc) = prototype_layout(serial) else {
            return false;
        };
        if !JSValue::from_bits(bits).is_pointer() {
            return false;
        }
        // Sealed with its serial, so every write since (a store, a define, a
        // delete, a relink) set its shadow bit.
        let word = (*JSValue::from_bits(bits).as_pointer::<ObjectHeader>()).parent_class_id;
        if !(crate::object::shapes::SHAPE_ID_BASE..crate::object::layout_desc::UNSHADOWED_WORD_END)
            .contains(&word)
        {
            return false;
        }
        family = (*desc).proto;
    }
}

/// A fresh object on `layout`, every slot the hole, or `None` when `proto`
/// does not let the birth use it (see [`sealed_chain`]). Can collect.
///
/// # Safety
/// As [`sealed_chain`].
pub(crate) unsafe fn alloc_on_layout(
    proto: f64,
    layout: *const PerryLayoutDesc,
) -> Option<*mut ObjectHeader> {
    sealed_chain(proto, layout).then(|| crate::object::layout_desc::perry_alloc_layout(layout))
}

/// Store `value` into slot `slot` of a newborn `obj` that nothing has
/// observed: the constructor creating an own data property node's order puts
/// at this position (`key_view::fill_hole` decides in-place or append).
///
/// # Safety
/// `obj` is live and born on a stream instance layout holding `slot`.
#[inline]
pub(crate) unsafe fn birth_store(obj: *mut ObjectHeader, slot: u32, value: f64) {
    // Still exactly on its instance layout (no shadow bit: no overflow, no
    // attribute variant): the layout's keys name each slot, so one load per
    // later key decides whether node's order puts the key here.
    if let Some(layout) = instance_layout(obj) {
        let keys = std::slice::from_raw_parts((*layout).keys, (*layout).key_count as usize);
        if let Some(position) = keys.iter().position(|k| k.slot == slot) {
            let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            if !keys[position + 1..]
                .iter()
                .any(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE)
            {
                crate::object::store_object_field_slot(obj, slot as usize, value.to_bits());
                return;
            }
        }
    }
    crate::object::key_view::fill_hole(obj, slot, value.to_bits());
}

/// The instance layout `obj` was born on, when it is one of these.
///
/// # Safety
/// `obj` is a live object header.
pub(crate) unsafe fn instance_layout(obj: *const ObjectHeader) -> Option<*const PerryLayoutDesc> {
    use crate::object::runtime_layouts as rl;
    let word = (*obj).parent_class_id;
    [
        &raw const rl::EVENT_EMITTER,
        &raw const rl::LEGACY_STREAM,
        &raw const rl::READABLE,
        &raw const rl::WRITABLE,
        &raw const rl::DUPLEX,
        &raw const rl::TRANSFORM,
        &raw const rl::PASSTHROUGH,
        &raw const rl::SERVER_RESPONSE,
    ]
    .into_iter()
    .find(|&desc| (*desc).layout_id == word)
}

/// A constructor's store of `name` into an object born on a stream instance
/// layout whose slot for it holds the hole: the own-property creation
/// [`birth_store`] makes, while the object's prototype chain still is the
/// sealed, unwritten one its layout names (so no inherited setter or
/// read-only data stands in front). False when `obj` is not such an object
/// or the key is not its hole; the ordinary `[[Set]]` then runs.
///
/// # Safety
/// `obj` is a live object header.
pub(crate) unsafe fn layout_birth_store(obj: *mut ObjectHeader, name: &[u8], value: f64) -> bool {
    let Some(layout) = instance_layout(obj) else {
        return false;
    };
    let keys = std::slice::from_raw_parts((*layout).keys, (*layout).key_count as usize);
    let Some(key) = keys.iter().find(|key| {
        let s = (*key.atom).string;
        (*key.atom).kind == crate::codegen_abi::ATOM_KIND_STRING
            && std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize)
                == name
    }) else {
        return false;
    };
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    if *fields.add(key.slot as usize) != crate::value::TAG_HOLE {
        return false;
    }
    let proto = f64::from_bits(crate::object::shapes::object_prototype_word(obj));
    if !sealed_chain(proto, layout) {
        return false;
    }
    birth_store(obj, key.slot, value);
    true
}

/// The slot of `_readableState` (`readable`) or `_writableState` of `obj`
/// when its layout word is exactly one of the stream instance layouts (no
/// shadow bit: no overflow, no attribute variant), so the key is plain own
/// data there or a hole. `None` for every other object.
///
/// # Safety
/// `obj` is a live object header.
#[inline]
pub(crate) unsafe fn state_object_slot(obj: *const ObjectHeader, readable: bool) -> Option<u32> {
    use crate::object::runtime_layouts::{self as rl, duplex_slot, readable_slot, writable_slot};
    let word = (*obj).parent_class_id;
    let duplex = word == rl::DUPLEX.layout_id
        || word == rl::TRANSFORM.layout_id
        || word == rl::PASSTHROUGH.layout_id;
    if readable {
        (duplex || word == rl::READABLE.layout_id).then_some(readable_slot::STATE)
    } else if duplex {
        Some(duplex_slot::WRITABLE_STATE)
    } else {
        (word == rl::WRITABLE.layout_id).then_some(writable_slot::STATE)
    }
}
