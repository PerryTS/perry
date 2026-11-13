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
use crate::codegen_abi::LayoutWord;
use crate::object::layout_desc::{PerryLayoutDesc, LAYOUT_PROTO_NULL, LAYOUT_PROTO_OBJECT};
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// A runtime layout's fixed word and the descriptor a match reads. Identity
/// is the word, never the descriptor: a program that does not reach a layout
/// defines no descriptor for it, and naming one is a birth (`desc()` aborts).
/// A matching word proves the layout was born, so `desc()` cannot abort there.
type LayoutEntry = (u32, fn() -> *mut PerryLayoutDesc);

/// The declared layout of the sealed node prototype of intrinsic `serial`.
fn prototype_layout(serial: u64) -> Option<LayoutEntry> {
    use crate::closure::shape::*;
    use crate::object::runtime_layouts as rl;
    Some(match serial {
        INTRINSIC_SERIAL_EVENT_EMITTER_PROTOTYPE => (
            rl::EVENT_EMITTER_PROTOTYPE::WORD,
            rl::EVENT_EMITTER_PROTOTYPE::desc,
        ),
        INTRINSIC_SERIAL_STREAM_PROTOTYPE => {
            (rl::STREAM_PROTOTYPE::WORD, rl::STREAM_PROTOTYPE::desc)
        }
        INTRINSIC_SERIAL_READABLE_PROTOTYPE => {
            (rl::READABLE_PROTOTYPE::WORD, rl::READABLE_PROTOTYPE::desc)
        }
        INTRINSIC_SERIAL_WRITABLE_PROTOTYPE => {
            (rl::WRITABLE_PROTOTYPE::WORD, rl::WRITABLE_PROTOTYPE::desc)
        }
        INTRINSIC_SERIAL_DUPLEX_PROTOTYPE => {
            (rl::DUPLEX_PROTOTYPE::WORD, rl::DUPLEX_PROTOTYPE::desc)
        }
        INTRINSIC_SERIAL_TRANSFORM_PROTOTYPE => {
            (rl::TRANSFORM_PROTOTYPE::WORD, rl::TRANSFORM_PROTOTYPE::desc)
        }
        INTRINSIC_SERIAL_PASSTHROUGH_PROTOTYPE => (
            rl::PASSTHROUGH_PROTOTYPE::WORD,
            rl::PASSTHROUGH_PROTOTYPE::desc,
        ),
        INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE => (
            rl::OUTGOING_MESSAGE_PROTOTYPE::WORD,
            rl::OUTGOING_MESSAGE_PROTOTYPE::desc,
        ),
        INTRINSIC_SERIAL_SERVER_RESPONSE_PROTOTYPE => (
            rl::SERVER_RESPONSE_PROTOTYPE::WORD,
            rl::SERVER_RESPONSE_PROTOTYPE::desc,
        ),
        INTRINSIC_SERIAL_INCOMING_MESSAGE_PROTOTYPE => (
            rl::INCOMING_MESSAGE_PROTOTYPE::WORD,
            rl::INCOMING_MESSAGE_PROTOTYPE::desc,
        ),
        _ => return None,
    })
}

/// Is `proto` the sealed prototype `layout` names, with every prototype of
/// its chain unwritten since sealing?
///
/// # Safety
/// `layout` is a registered instance layout naming a node prototype family.
pub(crate) unsafe fn sealed_chain(proto: f64, layout: *const PerryLayoutDesc) -> bool {
    let family = (*layout).proto;
    match chain_end(family) {
        Some(end) => end,
        None => sealed_prototype_chain(proto, (*family).serial),
    }
}

/// The answer at a chain's end family: `%Object.prototype%` unwritten, or a
/// null prototype. `None` for a node prototype family.
fn chain_end(family: *const crate::object::layout_desc::PerryProtoFamily) -> Option<bool> {
    if family == LAYOUT_PROTO_OBJECT {
        return Some(crate::object::builtin_proto::pristine(
            crate::object::builtin_proto::BuiltinProto::Object,
        ));
    }
    (family == LAYOUT_PROTO_NULL).then_some(true)
}

/// [`sealed_chain`] from the sealed node prototype of intrinsic `serial`:
/// `proto` is that prototype, and it and every prototype after it are on
/// their layouts, unwritten. Names no instance layout, and a prototype
/// layout's descriptor only after the prototype's word matched it.
///
/// # Safety
/// `proto` is a live value's bits.
unsafe fn sealed_prototype_chain(proto: f64, mut serial: u64) -> bool {
    let mut first = true;
    loop {
        let bits = crate::object::shapes::identity_prototype_word(serial);
        if first && bits != proto.to_bits() {
            return false;
        }
        first = false;
        let Some((layout_word, desc)) = prototype_layout(serial) else {
            return false;
        };
        if !JSValue::from_bits(bits).is_pointer() {
            return false;
        }
        // Sealed with its serial, so every write since (a store, a define, a
        // delete, a relink) set its shadow bit: an unwritten prototype still
        // has its layout's word. That match also lets `desc()` run: the
        // prototype was born on the layout, so its descriptor is defined.
        let word = (*JSValue::from_bits(bits).as_pointer::<ObjectHeader>()).parent_class_id;
        if word != layout_word
            || !(crate::object::shapes::SHAPE_ID_BASE
                ..crate::object::layout_desc::UNSHADOWED_WORD_END)
                .contains(&word)
        {
            return false;
        }
        let family = (*desc()).proto;
        if let Some(end) = chain_end(family) {
            return end;
        }
        serial = (*family).serial;
    }
}

/// The instance layout node's constructor of the sealed prototype of
/// intrinsic `serial` gives its instances. A program that reaches the
/// prototype reaches this layout too (`new (proto.constructor)()` and a
/// derived `new F` birth on it), so after the chain matches, `desc()` is that
/// birth.
fn instance_layout_of_prototype(serial: u64) -> Option<LayoutEntry> {
    use crate::closure::shape::*;
    use crate::object::runtime_layouts as rl;
    Some(match serial {
        INTRINSIC_SERIAL_EVENT_EMITTER_PROTOTYPE => {
            (rl::EVENT_EMITTER::WORD, rl::EVENT_EMITTER::desc)
        }
        INTRINSIC_SERIAL_STREAM_PROTOTYPE => (rl::LEGACY_STREAM::WORD, rl::LEGACY_STREAM::desc),
        INTRINSIC_SERIAL_READABLE_PROTOTYPE => (rl::READABLE::WORD, rl::READABLE::desc),
        INTRINSIC_SERIAL_WRITABLE_PROTOTYPE => (rl::WRITABLE::WORD, rl::WRITABLE::desc),
        INTRINSIC_SERIAL_DUPLEX_PROTOTYPE => (rl::DUPLEX::WORD, rl::DUPLEX::desc),
        INTRINSIC_SERIAL_TRANSFORM_PROTOTYPE => (rl::TRANSFORM::WORD, rl::TRANSFORM::desc),
        INTRINSIC_SERIAL_PASSTHROUGH_PROTOTYPE => (rl::PASSTHROUGH::WORD, rl::PASSTHROUGH::desc),
        INTRINSIC_SERIAL_SERVER_RESPONSE_PROTOTYPE => {
            (rl::SERVER_RESPONSE::WORD, rl::SERVER_RESPONSE::desc)
        }
        _ => return None,
    })
}

/// Most user prototype levels a derived chain may have before its native
/// ancestor (`util.inherits` chains are one or two deep).
const DERIVED_LEVELS: usize = 8;

/// The instance layout of the nearest sealed node prototype on the chain
/// that starts at `proto` (a `new F`'s `F.prototype`), when every level
/// before it is plain (an ordinary shape with no accessor or read-only key
/// and no overflow) and its own chain is unwritten ([`sealed_chain`]). Such a
/// chain intercepts none of the layout's keys, so the native constructor's
/// `[[Set]]`s on an instance are own-property creations in its order: the
/// instance can be born on the layout's keys with `proto` as its prototype
/// (`layout_desc::alloc_derived_layout_object`). One shape-record load per
/// plain level, then [`sealed_chain`]. `None` for every other chain.
///
/// # Safety
/// `proto` is a live value's bits.
pub(crate) unsafe fn derived_layout(proto: u64) -> Option<*const PerryLayoutDesc> {
    derived_layout_entry(proto).map(|(_, desc)| desc() as *const PerryLayoutDesc)
}

/// [`derived_layout`] as the layout's word and descriptor: a probe compares
/// the word and names no descriptor.
///
/// # Safety
/// As [`derived_layout`].
unsafe fn derived_layout_entry(proto: u64) -> Option<LayoutEntry> {
    use crate::object::key_attrs::SUMMARY_BLOCKS_STORE;
    use crate::object::shapes::ShapeObjectKind;
    let mut bits = proto;
    for _ in 0..DERIVED_LEVELS {
        let value = JSValue::from_bits(bits);
        if !value.is_pointer() {
            return None;
        }
        let obj = value.as_pointer::<ObjectHeader>();
        let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
        if crate::object_lacks_meta_word!(header.obj_type)
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let meta = crate::meta_read!(crate::meta_obj_from_type!(obj as *mut _, header.obj_type));
        if !meta.is_null() {
            if let Some(entry) = instance_layout_of_prototype((*meta).proto_serial) {
                return sealed_prototype_chain(f64::from_bits(bits), (*meta).proto_serial)
                    .then_some(entry);
            }
        }
        if crate::object::overflow::has_overflow(obj) {
            return None;
        }
        let record = crate::object::shapes::object_shape_record(obj)?;
        if !matches!(
            record.object_kind(),
            ShapeObjectKind::Ordinary | ShapeObjectKind::OrdinaryUnmarked
        ) || record.summary() & SUMMARY_BLOCKS_STORE != 0
        {
            return None;
        }
        // A default link reaches %Object.prototype%: no native ancestor.
        bits = crate::object::shapes::object_prototype_word(obj);
        if bits == 0 {
            return None;
        }
    }
    None
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

/// Every runtime instance layout of a node constructor: its fixed word, and
/// the descriptor a match reads. Identity is the word, never the descriptor:
/// a program that does not reach a layout defines no descriptor for it, and
/// naming one is a birth (`desc()` aborts). A matching word proves the layout
/// was born, so its descriptor is defined and `desc()` cannot abort there.
const INSTANCE_LAYOUTS: [(u32, fn() -> *mut PerryLayoutDesc); 8] = {
    use crate::object::runtime_layouts as rl;
    [
        (rl::EVENT_EMITTER::WORD, rl::EVENT_EMITTER::desc),
        (rl::LEGACY_STREAM::WORD, rl::LEGACY_STREAM::desc),
        (rl::READABLE::WORD, rl::READABLE::desc),
        (rl::WRITABLE::WORD, rl::WRITABLE::desc),
        (rl::DUPLEX::WORD, rl::DUPLEX::desc),
        (rl::TRANSFORM::WORD, rl::TRANSFORM::desc),
        (rl::PASSTHROUGH::WORD, rl::PASSTHROUGH::desc),
        (rl::SERVER_RESPONSE::WORD, rl::SERVER_RESPONSE::desc),
    ]
};

/// The instance layout `obj` is on, when it is one of these: born on it
/// (its word is the layout's), or born on its keys with another prototype
/// (a `new F` with a native ancestor, [`derived_layout`]: its shape has the
/// layout's key list). No shadow bit either way: no overflow, no attribute
/// variant, so the layout's keys name every own key's slot.
///
/// # Safety
/// `obj` is a live object header.
pub(crate) unsafe fn instance_layout(obj: *const ObjectHeader) -> Option<*const PerryLayoutDesc> {
    let word = (*obj).parent_class_id;
    if let Some(&(_, desc)) = INSTANCE_LAYOUTS.iter().find(|&&(layout, _)| layout == word) {
        return Some(desc() as *const PerryLayoutDesc);
    }
    derived_instance_layout(LayoutWord(word))
}

/// [`instance_layout`] of a shape id that is not a layout's own: the layout
/// whose key list its shape has. A layout that was never born has no shape
/// record under its word, so it never matches and its descriptor is never
/// named.
#[inline(never)]
unsafe fn derived_instance_layout(word: LayoutWord) -> Option<*const PerryLayoutDesc> {
    // A derived shape is minted in the dynamic band; a static id (every
    // registered layout, a class family's) is its own layout or none.
    if word.0 & crate::object::layout_desc::LAYOUT_SHADOW_BIT != 0
        || crate::object::shapes::is_static_layout_word(word.0)
    {
        return None;
    }
    let (keys, count, _) = crate::object::shapes::shape_keys_and_proto_for_word(word)?;
    if keys == 0 {
        return None;
    }
    INSTANCE_LAYOUTS
        .iter()
        .find(|&&(layout, _)| {
            crate::object::shapes::shape_keys_and_proto_for_word(LayoutWord(layout)).is_some_and(
                |(layout_keys, layout_count, _)| layout_count == count && layout_keys == keys,
            )
        })
        .map(|&(_, desc)| desc() as *const PerryLayoutDesc)
}

/// The storage slot of own key `bytes` (hash `hash`, computed here when
/// `None`) of an object on an instance layout ([`instance_layout`]):
/// `Some(Some(slot))` when the layout names the key as plain data (its slot
/// may hold the hole: absent), `Some(None)` when it names no such key (no own
/// key of that name), `None` when `obj` is on no instance layout or the key
/// has attributes. One probe of the layout's hash index, no key-list scan.
///
/// # Safety
/// `obj` is a live object header; `hash`, when given, is `bytes`' key hash.
pub(crate) unsafe fn instance_layout_slot(
    obj: *const ObjectHeader,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<u32>> {
    let layout = &*instance_layout(obj)?;
    let hash = hash.unwrap_or_else(|| crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len()));
    match crate::object::layout_desc::desc_position_hashed(layout, bytes, hash) {
        None => Some(None),
        Some(pos) => {
            let key = &*layout.keys.add(pos as usize);
            (key.attrs & !crate::object::layout_desc::LAYOUT_KEY_LATE == 0)
                .then_some(Some(key.slot))
        }
    }
}

/// Is `obj` on the layout of word `word` with a prototype chain that intercepts none of its
/// keys: born on it with its sealed prototype unwritten ([`sealed_chain`]),
/// or born on its keys under a derived chain ([`derived_layout`])? A native
/// constructor's init asks once, then fills slots.
///
/// # Safety
/// `obj` is a live object header.
pub(crate) unsafe fn on_layout_with_plain_chain(obj: *const ObjectHeader, word: u32) -> bool {
    // `instance_layout` names a descriptor only after a word matched it.
    let Some(layout) = instance_layout(obj).filter(|&layout| (*layout).layout_id == word) else {
        return false;
    };
    let proto = crate::object::shapes::object_prototype_word(obj);
    if (*obj).parent_class_id == word {
        sealed_chain(f64::from_bits(proto), layout)
    } else {
        derived_layout_entry(proto).is_some_and(|(derived, _)| derived == word)
    }
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
    if !on_layout_with_plain_chain(obj, (*layout).layout_id) {
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
    let duplex =
        word == rl::DUPLEX::WORD || word == rl::TRANSFORM::WORD || word == rl::PASSTHROUGH::WORD;
    if readable {
        (duplex || word == rl::READABLE::WORD).then_some(readable_slot::STATE)
    } else if duplex {
        Some(duplex_slot::WRITABLE_STATE)
    } else {
        (word == rl::WRITABLE::WORD).then_some(writable_slot::STATE)
    }
}
