//! The shadow bit (object model step 3, `OBJECT_MODEL_2026-10-13.md` §3.6).
//!
//! Compiled code binds a prototype constant (a class method the program
//! never writes by name) as a direct call, and reads inherited data through a
//! known holder. Both rely on two facts about the objects involved: the
//! receiver has no own property of the bound name, and no prototype on the
//! path changed that name or its own [[Prototype]]. Static writes are the
//! compiler's (`perry_hir::proto_census`: a name written anywhere is never
//! bound). What the compiler cannot see is a write whose key or target is
//! data: those set bit 30 of the written object's layout word
//! ([`super::layout_desc::LAYOUT_SHADOW_BIT`]), and a site that binds a name
//! tests the bit on the receiver and on each prototype of its static chain.
//!
//! The bit summarizes the object's own contents. It is set when
//!
//! * a **shadow name** is added to, deleted from or redefined on the object
//!   ([`publish_word`], called by every structural publication of a layout
//!   word), or, on a marked prototype, takes a new value in place
//!   ([`note_prototype_slot_store`]; replacing a method body publishes no
//!   layout). The section `perry_shadow_names` holds every class member
//!   name; on a prototype each of them counts (so a clear bit proves every
//!   member slot still holds what the class definition stored, which lets a
//!   site that proved its receiver's exact layout call the declared body of
//!   a name the program also writes elsewhere), on a class instance or an
//!   object with a recorded [[Prototype]] only the names marked as receiver
//!   names (the census's constant members, when a dynamic write exists);
//! * the object's [[Prototype]] changes ([`note_relink`]);
//! * the object is an intrinsic prototype and is written at all
//!   (`builtin_proto::note_family_write`): `Pristine(family)`, which
//!   builtin-family sites test.
//!
//! It is never cleared (a delete may leave it set: one wasted generic call,
//! never a wrong answer), it survives every restamp
//! ([`super::layout_desc::carry_shadow`]), and it is not learned or validated
//! state: nothing reads it but the site compare, and nothing is recorded
//! anywhere else. Static writes never add a shadow name (the census makes a
//! written name a non-constant, so no site binds it); program code that
//! adds one is therefore always a dynamic write, wherever in the runtime the
//! write lands.

use super::layout_desc::{
    carry_shadow, layout_id_of_word, word_is_shadowed, StaticAtom, LAYOUT_SHADOW_BIT,
};
use super::shapes::{is_layout_word, is_site_matchable_layout_word};
use super::ObjectHeader;
use crate::codegen_abi::LayoutWord;
use crate::object::shapes::shape_record_of_receiver;

/// A view of compiler-owned data, never runtime-learned state.
#[derive(Clone, Copy)]
struct Names {
    masks: &'static [[u64; 64]; 2],
    records: &'static [StaticAtom],
    index: &'static [u32],
}

/// Layout shared with the compiler's constant shadow-name data.
#[repr(C)]
struct NameData {
    masks: [[u64; 64]; 2],
    index: *const u32,
    records: *const StaticAtom,
    index_mask: u64,
}

#[inline]
fn names() -> Option<Names> {
    let record = super::layout_desc::shadow_name_descriptor();
    if record.hash == 0 {
        return None;
    }
    let data = unsafe { &*record.string.cast::<NameData>() };
    Some(unsafe {
        Names {
            masks: &data.masks,
            records: std::slice::from_raw_parts(data.records, record.hash as usize),
            index: std::slice::from_raw_parts(data.index, data.index_mask as usize + 1),
        }
    })
}

#[inline(always)]
pub(crate) fn names_present() -> bool {
    super::layout_desc::shadow_name_descriptor().hash != 0
}

/// Source-name membership, read from the program's constant descriptor.
/// Receiver admission uses only constant prototype methods and accessors;
/// prototype admission includes every class member.
#[inline(always)]
pub(crate) fn is_shadow_name(bytes: &[u8], prototype: bool) -> bool {
    let Some(names) = names() else { return false };
    mask_admits(names, bytes, prototype) && name_matches_index(names, bytes, prototype)
}

/// The immutable length/prefix mask: false proves `bytes` is no shadow name.
#[inline(always)]
fn mask_admits(names: Names, bytes: &[u8], prototype: bool) -> bool {
    let row = bytes.len() & 63;
    let prefix = u64::from(bytes.first().copied().unwrap_or(0)) * 33
        ^ u64::from(bytes.get(1).copied().unwrap_or(0));
    let bit = 1u64 << (prefix & 63);
    names.masks[usize::from(prototype)][row] & bit != 0
}

/// Full spelling comparison is reached only after the immutable admission
/// mask matched. Keep its loop/register needs off the common negative path.
#[cold]
#[optimize(size)]
#[inline(never)]
fn name_matches_index(names: Names, bytes: &[u8], prototype: bool) -> bool {
    let mut h = (bytes.len() as u64) << 48;
    if bytes.len() >= 6 {
        // Read only bytes the spelling owns. Unaligned loads avoid hashing
        // every byte of long, unrelated constructor-field names.
        unsafe {
            h |= u64::from(u32::from_le(std::ptr::read_unaligned(
                bytes.as_ptr().cast::<u32>(),
            )));
            h |= u64::from(u16::from_le(std::ptr::read_unaligned(
                bytes.as_ptr().add(4).cast::<u16>(),
            ))) << 32;
        }
    } else {
        for (i, byte) in bytes.iter().enumerate() {
            h |= u64::from(*byte) << (8 * i);
        }
    }
    h &= !1;
    let mask = names.index.len() - 1;
    let mut i = (((h ^ (h >> 32)).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 32) as usize) & mask;
    loop {
        let position = names.index[i];
        if position == u32::MAX {
            return false;
        }
        let record = &names.records[position as usize];
        if record.hash & !1 == h && (prototype || record.hash & 1 != 0) {
            let string = record.string;
            let candidate = unsafe {
                std::slice::from_raw_parts(
                    crate::string::string_data(string),
                    (*string).byte_len as usize,
                )
            };
            if candidate == bytes {
                return true;
            }
        }
        i = (i + 1) & mask;
    }
}

/// Is the key value `bits` (a keys-array element) a shadow name?
#[inline(always)]
unsafe fn key_bits_is_shadow_name(bits: u64, prototype: bool, receiver_class: u32) -> bool {
    // Shape keys are constructed as heap STRING_TAG values (see
    // JSValue::is_string's stored-key contract). Their construction proves
    // the header, so no allocator admission or generic key conversion runs.
    let value = crate::JSValue::from_bits(bits);
    if !value.is_string() {
        return false; // symbol/private entries are not string names
    }
    let string = value.as_string_ptr();
    let bytes = std::slice::from_raw_parts(
        crate::string::string_data(string),
        (*string).byte_len as usize,
    );
    is_shadow_name(bytes, prototype)
        && (receiver_class == 0
            || receiver_family_declares_name(receiver_class, bytes).unwrap_or(true))
}

/// Receiver shadowing is relative to its family's declared chain. A field
/// whose name is a method only in another family cannot shadow this family's
/// binding. Declarations are immutable image metadata, never learned state.
fn receiver_family_declares_name(mut class: u32, name: &[u8]) -> Option<bool> {
    for _ in 0..64 {
        class = super::class_generic_origin(class).unwrap_or(class);
        let decl = super::class_registry::declarations::class_declaration(class)?;
        if decl.has_computed_members() {
            return None;
        }
        if decl
            .members()
            .iter()
            .any(|member| member.name_bytes() == Some(name))
        {
            return Some(true);
        }
        class = super::class_registry::get_parent_class_id(class).unwrap_or(0);
        if class == 0 {
            return Some(false);
        }
    }
    None
}

/// Keys `[from, to)` of a shape's keys array: does any name a shadow name?
#[inline(always)]
unsafe fn keys_hold_shadow_name(
    keys: u64,
    from: usize,
    to: usize,
    prototype: bool,
    receiver_class: u32,
) -> bool {
    if keys == 0 {
        return false;
    }
    // These are live shape-record keys read on this noncollecting path.
    // Their traced owner has already resolved the array header.
    let (slots, len) = super::keys_lookup::keys_array_dense_slots_resolved(
        keys as usize as *const crate::array::ArrayHeader,
    );
    if slots.is_null() {
        return false;
    }
    (from..to.min(len))
        .any(|i| key_bits_is_shadow_name((*slots.add(i)).to_bits(), prototype, receiver_class))
}

/// Does restamping a shaped object from `prev` to `id` add, remove or
/// redefine a shadow name?
///
/// A different [[Prototype]] identity alone is not a content change. The
/// links that give an object its [[Prototype]] at birth (construction, a
/// class evaluation, `Object.create`, runtime wiring) are not writes, and the
/// one link that changes a live object's chain (`setPrototypeOf`,
/// `__proto__`) sets the bit in the link funnel ([`note_relink`]) before it
/// restamps.
unsafe fn change_touches_shadow_name(
    prev: LayoutWord,
    id: LayoutWord,
    any_member: bool,
    receiver_class: u32,
) -> bool {
    // Both words are carried: `prev` is the receiver's header word and `id`
    // the live layout the stamp funnel is storing into that header.
    let (Some(a), Some(b)) = (
        shape_record_of_receiver((prev).0),
        shape_record_of_receiver((id).0),
    ) else {
        return false;
    };
    let (ka, na) = (a.keys(), a.logical_key_count() as usize);
    let (kb, nb) = (b.keys(), b.logical_key_count() as usize);
    if ka == kb && na == nb {
        // The same key list: a representation, kind or twin restamp, or an
        // attribute change, which shows in the attribute summary.
        return a.summary() != b.summary()
            && keys_hold_shadow_name(ka, 0, na, any_member, receiver_class);
    }
    if nb > na {
        // Keys are only ever appended (insertion order): a key add adds
        // exactly the tail.
        return keys_hold_shadow_name(kb, na, nb, any_member, receiver_class);
    }
    // A delete, or an attribute change (same count, new list): any shadow
    // name the object had.
    if nb == na {
        // The same count on another list: an attribute change, or a
        // restamp that renames nothing. Only the positions whose key differs
        // can matter; a plain pointer compare skips the rest.
        return keys_differing_hold_shadow_name(ka, kb, na, any_member, receiver_class);
    }
    // A delete. Every member name counts here: a deleted field is what a
    // known-holder read relies on.
    keys_hold_shadow_name(ka, 0, na, any_member, receiver_class)
}

/// Positions `< n` where the two key lists differ: does either side name a
/// member? (An attribute change re-lists the same key with new attributes,
/// which is a different element.)
unsafe fn keys_differing_hold_shadow_name(
    ka: u64,
    kb: u64,
    n: usize,
    any_member: bool,
    receiver_class: u32,
) -> bool {
    let (sa, la) = super::keys_lookup::keys_array_dense_slots_resolved(
        ka as usize as *const crate::array::ArrayHeader,
    );
    let (sb, lb) = super::keys_lookup::keys_array_dense_slots_resolved(
        kb as usize as *const crate::array::ArrayHeader,
    );
    if sa.is_null() || sb.is_null() {
        return keys_hold_shadow_name(ka, 0, n, any_member, receiver_class)
            || keys_hold_shadow_name(kb, 0, n, any_member, receiver_class);
    }
    (0..n.min(la).min(lb)).any(|i| {
        let (a, b) = ((*sa.add(i)).to_bits(), (*sb.add(i)).to_bits());
        a != b
            && (key_bits_is_shadow_name(a, any_member, receiver_class)
                || key_bits_is_shadow_name(b, any_member, receiver_class))
    })
}

/// The layout word a restamp of `obj` from the word `previous` to the
/// ShapeId `id` publishes: [`carry_shadow`], plus the bit when the restamp
/// adds, removes or redefines a shadow name. Births (no ShapeId yet) and
/// birth links never set it; a relink is marked by [`note_relink`].
#[inline(always)]
pub(crate) unsafe fn publish_word(
    obj: *const ObjectHeader,
    previous: u32,
    id: LayoutWord,
) -> LayoutWord {
    let word = carry_shadow(previous, id);
    if word.0 != id.0 || previous == id.0 {
        return word;
    }
    // A1_UNPROVEN: record-first: shape word publish (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    let prototype = !meta.is_null() && (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE != 0;
    if !prototype
        && shape_record_of_receiver((id).0).is_some_and(|r| {
            matches!(
                r.proto_id(),
                super::shapes::PROTO_ID_DEFAULT | super::shapes::PROTO_ID_NULL
            )
        })
    {
        // The layout proves this object has no class chain to protect: its
        // [[Prototype]] is `Object.prototype` or null (a literal family's
        // class id implies no prototype; a class instance's identity is
        // `CLASS`/`MIXED`). Do not enter source-name admission for
        // default/null data bags.
        return word;
    }
    if !names_present() {
        return word;
    }
    publish_word_slow(obj, previous, id, prototype)
}

/// [`publish_word`] for an append of exactly `key` (the producer's transition
/// is keyed on it): admission tests that key, nothing is re-derived.
#[inline(always)]
pub(crate) unsafe fn publish_key_add_word(
    obj: *const ObjectHeader,
    previous: u32,
    id: LayoutWord,
    key: *const crate::StringHeader,
) -> LayoutWord {
    let word = carry_shadow(previous, id);
    if word.0 != id.0 || previous == id.0 || key.is_null() {
        return word;
    }
    // A1_UNPROVEN: record-first: shape word publish (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    let prototype = !meta.is_null() && (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE != 0;
    let Some(names) = names() else {
        return word;
    };
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    let any_member = prototype
        || (*obj).class_id == 0
        || (*obj).class_id >= super::class_registry::SYNTHETIC_CLASS_ID_BASE;
    // The immutable mask rejects almost every key before anything else runs.
    if !mask_admits(names, bytes, any_member) {
        return word;
    }
    key_add_slow(obj, previous, id, prototype, any_member, names, bytes)
}

#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn key_add_slow(
    obj: *const ObjectHeader,
    previous: u32,
    id: LayoutWord,
    prototype: bool,
    any_member: bool,
    names: Names,
    bytes: &[u8],
) -> LayoutWord {
    let prev = layout_id_of_word(previous);
    if prev == id.0 || !is_layout_word(prev) || !is_site_matchable_layout_word(id.0) {
        return id;
    }
    // A default/null-prototype data bag has no inherited binding to protect.
    if !prototype
        && shape_record_of_receiver((id).0).is_some_and(|r| {
            matches!(
                r.proto_id(),
                super::shapes::PROTO_ID_DEFAULT | super::shapes::PROTO_ID_NULL
            )
        })
    {
        return id;
    }
    if !name_matches_index(names, bytes, any_member) {
        return id;
    }
    let receiver_class = if any_member { 0 } else { (*obj).class_id };
    let shadows =
        receiver_class == 0 || receiver_family_declares_name(receiver_class, bytes).unwrap_or(true);
    if shadows && !super::own_override::in_builtin_definition() {
        LayoutWord(id.0 | crate::codegen_abi::LAYOUT_SHADOW_BIT)
    } else {
        id
    }
}

#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn publish_word_slow(
    obj: *const ObjectHeader,
    previous: u32,
    id: LayoutWord,
    prototype: bool,
) -> LayoutWord {
    let prev = layout_id_of_word(previous);
    if prev == id.0 || !is_layout_word(prev) || !is_site_matchable_layout_word(id.0) {
        return id;
    }
    // A class instance gains its declared fields by key adds at its birth
    // (`js_class_field_add`): an added key shadows something only when it
    // names a bound method (a receiver name). On a prototype or an object
    // with a recorded [[Prototype]] every member name counts.
    let any_member = prototype
        || (*obj).class_id == 0
        || (*obj).class_id >= super::class_registry::SYNTHETIC_CLASS_ID_BASE;
    let receiver_class = if any_member { 0 } else { (*obj).class_id };
    // The runtime's own definitions (lazy builtin installs) are not program
    // writes; that thread-local is read only once a name matched.
    if change_touches_shadow_name(LayoutWord(prev), id, any_member, receiver_class)
        && !super::own_override::in_builtin_definition()
    {
        LayoutWord(id.0 | crate::codegen_abi::LAYOUT_SHADOW_BIT)
    } else {
        id
    }
}

/// Set the bit on `obj`. Only an ordinary-band ShapeId can carry it; any
/// other word already fails every site test (dictionary, exotic) or names
/// no layout a site binds (an unstamped allocation word).
#[inline]
pub(crate) unsafe fn set_bit(obj: *mut ObjectHeader) {
    let w = (*obj).parent_class_id;
    if is_site_matchable_layout_word(w) {
        (*obj).parent_class_id = w | LAYOUT_SHADOW_BIT;
    }
}

/// The ordinary object at `addr`, if it is one.
unsafe fn ordinary(addr: usize) -> Option<*mut ObjectHeader> {
    if addr == 0 || !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    (crate::gc_type_is_object!(header.obj_type)).then_some(addr as *mut ObjectHeader)
}

/// `addr`'s [[Prototype]] changed (the prototype funnel): what any binding
/// site proved about its chain no longer holds.
pub(crate) fn note_relink(addr: usize) {
    unsafe {
        if let Some(obj) = ordinary(addr) {
            set_bit(obj);
        }
    }
}

/// A value store into inline slot `field_index` (or spill slot
/// `field_index` when `spill`) of a marked prototype after its creation: a
/// method body replaced in place publishes no layout, so a class-member-name
/// slot sets the bit here.
/// Called from the slot-store funnels behind their prototype-flag test.
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) unsafe fn note_prototype_slot_store(
    obj: *mut ObjectHeader,
    field_index: usize,
    spill: bool,
) {
    if !names_present()
        || super::own_override::in_builtin_definition()
        || word_is_shadowed((*obj).parent_class_id)
    {
        return;
    }
    let Some(record) = shape_record_of_receiver((*obj).shape_word()) else {
        return;
    };
    let n = record.logical_key_count() as usize;
    // A spill slot `i` holds the key at position `live_inline + i`.
    let at = if spill {
        record.live_inline_slot_count() as usize + field_index
    } else {
        field_index
    };
    if at < n && keys_hold_shadow_name(record.keys(), at, at + 1, true, 0) {
        set_bit(obj);
    }
}
