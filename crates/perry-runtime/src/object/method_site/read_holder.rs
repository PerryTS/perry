//! The read site's HOLDER entry: `o.k` where `k` is not an own key of the
//! receiver, answered by facts of two shapes.
//!
//! * The receiver's ShapeId `S` vouches that `k` is not own, that the receiver
//!   is an ordinary object, and its [[Prototype]] identity. A serial, default
//!   (the realm's `Object.prototype`), or MIXED explicit identity pins ONE
//!   object; null terminates the chain. A collecting accessor entry may
//!   also use a declared class identity: the registry link from a class to
//!   its declared prototype is written once, and a write that replaces or
//!   redirects it retires the displaced prototype's ShapeId
//!   (`class_registry::retire_displaced_decl_prototype`), so the holder-shape
//!   compare below sees the relink.
//! * The holder's ShapeId `SH` vouches that `k` is an own inline data slot of
//!   the holder `H` — or, for an ABSENT entry, that the terminal object lacks
//!   `k` and has a null [[Prototype]].
//! * For a holder deeper than the direct prototype, each intermediate hop's
//!   ShapeId vouches that the hop lacks `k` and still links to the next hop.
//!
//! Data/absence facts are compared on use without a global invalidation word:
//! a key add, delete, descriptor change or `setPrototypeOf` on any object
//! the entry names moves that object's ShapeId, and a value store to the
//! holder's slot is seen because the hit LOADS the slot. A delete is a shape
//! transition (#10826), so a holder whose ShapeId matches still has the slot:
//! the hit needs no `TAG_HOLE` test, as the emitted MRU hit needs none. The
//! collecting accessor route is the same two compares: the holder's ShapeId
//! declares the key an accessor lane (`ENTRY_ACCESSOR` in its key list), and
//! the hit loads the lane and compares it with the pair it was primed with,
//! which names the getter it then calls: a compiled class getter through its
//! code address, any other function object (a compiled function body or a
//! builtin thunk) through the closure ABI.
//!
//! The primary entry lives in `PicCache` words [`HOLDER_RECV`]..[`HOLDER_STATE`].
//! Eight further complete answers belong to the same site's bounded record.
//!
//! # Weak entries
//!
//! No entry word is a GC root, no site is registered, and no collection visits
//! or rewrites an entry. The same holds for the class entries
//! (`class_read.rs`), keyed entries' holders (`keyed.rs`; their KEYS are still
//! rooted until keys are immortal atoms) and method-site entries and chain
//! proofs (`method_site.rs`). Each premise below is enforced at publication
//! and backed by a test (`gc/tests/holder_memo.rs`):
//!
//! 1. **Stable referents (I1).** Every object address an entry stores — a
//!    holder, an intermediate hop, a terminal — is admitted by
//!    `method_site::memo_referents_stable`: a `GC_TYPE_OBJECT` that is old and
//!    never-move (`gc::never_move`). A minor never moves an old object, and
//!    the only passes that move old objects (old-page defragmentation and
//!    idle compaction) decline any block holding a never-move one. So while a
//!    named object lives, it is at the stored address. A young candidate is
//!    refused and marked; the next minor promotes it and the site primes again.
//! 2. **ShapeIds are never reused** (`shapes.rs`), and a hit reads no entry
//!    word before the live receiver's ShapeId equals the entry's token.
//! 3. **A receiver ShapeId pins its [[Prototype]]**: a serial, default or
//!    MIXED identity names one object, and the receiver (its meta record, or
//!    the shape's identity word) references it, so a live receiver carrying
//!    the token keeps that prototype alive. Each further hop is admitted only
//!    if its own ShapeId pins its link (`hop_identity_pins_link`), and the hit
//!    compares each hop's ShapeId before following it, so by induction every
//!    named object is reachable from the receiver. A ShapeId whose objects
//!    have all died can never be carried again (2), so an entry naming a dead
//!    object can never pass its token compare.
//! 4. **A bare CLASS identity** names the class's *current* prototype, which a
//!    relink can displace while receivers of the token (or, for a declared
//!    parent link, a hop carrying it) live on. Class entries and chain proofs
//!    compare the live identity word before any holder. For every other
//!    reader, publication flags each referent memo-named
//!    (`gc::never_move::note_memo_named`) and the relink's retirement
//!    (`class_registry::retire_displaced_decl_prototype`) pins a flagged
//!    prototype, so the entry keeps reading that object, whose re-stamped
//!    ShapeId no longer matches.
//! 5. **Accessor pairs** are compared, never dereferenced before the compare:
//!    the hit loads the holder's live lane and compares it with the primed
//!    pair. A pair is reachable from its holder while it is in the lane.
//!    Replacing a pair's getter or setter moves the holder to a fresh ShapeId
//!    (`descriptor_state::note_accessor_function_replaced`), so a new pair
//!    reusing a dead pair's address in the same lane matches an old entry
//!    only if it holds the same functions — and then the entry's getter is
//!    that pair's getter. A pair that moves only makes the entry miss and
//!    re-prime.
//!
//! The worker gate is unchanged: once a worker starts, no agent reads an
//! entry, so stale words are inert.
//!
//! # Emitted form
//!
//! Emitted code holds nothing of the entry. The read site's ShapeId compare
//! misses, and its one GC-leaf front call
//! (`field_get_set::ic_miss::read_confirm::js_object_get_field_ic_front`)
//! asks [`entry_answer`] after the ways, the spill entry and a latched site's
//! confirm, so an own-key read pays nothing for it. A decline is `TAG_HOLE`
//! and the site continues to the collecting slow call. The emitted getter arm
//! expands the shared guard program before calling user code with the
//! original receiver. Once a worker starts,
//! the front declines before reading any holder-entry word: the entry belongs
//! to the primary heap and no agent's GC scans it after the gate.
//!
//! # Priming
//!
//! Only from the primary agent's read miss handler, which already knows the key
//! is not own. Data and absent entries are recorded only after the generic
//! getter's answer agrees with the shapes. An accessor is published only
//! after its pair is validated, then its getter runs once. A worker
//! agent's start gates all further holder hits and primes.
//!
//! A miss whose receiver a live entry already answers is served from the
//! entry and primes nothing. Complete answers for other receiver shapes use
//! bounded eviction in the existing site record. An unsupported receiver
//! leaves unrelated answers intact; it never permanently retires the site.

use super::{next_prototype, ordinary_receiver, WORKER_AGENTS_EXIST};
use crate::object::shapes::{
    object_shape_descriptor, object_shape_stamp, shape_proto_id, PIC_ID_TOKEN_BIT, PROTO_ID_CLASS,
    PROTO_ID_DEFAULT, PROTO_ID_MIXED, PROTO_ID_NULL, PROTO_ID_UNIQUE,
};
use crate::object::ObjectHeader;
use std::sync::atomic::Ordering;

mod accessor_path;
pub(crate) use accessor_path::accessor_walk;
mod inspect;
pub(crate) use inspect::shape_answer;
pub(crate) mod keyed;
pub(crate) mod shared;

/// The answer's kind:
///
/// | value | meaning |
/// |---|---|
/// | `0 ..= u32::MAX` | depth 1, the value is the holder's slot word ([`HOLDER_SLOT_SPILL`]) |
/// | [`HOLDER_ABSENT_DEPTH1`] | depth 1, absent: the answer is `undefined` |
/// | [`HOLDER_ACCESSOR`] + slot word | direct-prototype accessor; collecting hit only |
/// | [`HOLDER_MULTI_ABSENT`] | depth 1 for up to ten receiver shapes: absent ([`HOLDER_ABSENT_BIT`]) or the holder's slot word ([`multi_slot`]) |
/// | negative | [`HOLDER_STUB`] set: depth 2..=4 and/or a deep absent entry |
pub const HOLDER_KIND: usize = crate::codegen_abi::PIC_HOLDER_KIND_WORD;
/// First of three intermediate hop addresses (depth 2..=4). For accessors,
/// the first word is the immutable pair root, the second its native getter
/// code (a scalar used by non-observable probes), and the third is unused.
pub const HOLDER_HOPS: usize = HOLDER_KIND + 1;
/// Data/absence: first and second hop ShapeIds. Accessor: the code the hit
/// calls for the getter the primed pair names, `double get(double this, i64
/// pair)`: a compiled class getter, or the closure-getter entry
/// (`accessor_pair::holder_closure_getter_entry`), which reads the closure
/// from the pair at each hit; 0 for a setter-only pair or a deep chain
/// that needs the collecting validator. A code address, never a GC pointer:
/// no GC pass reads this word.
/// The accessor's pair itself (its raw address) is in [`HOLDER_HOPS`]: a
/// compared word, never a root (see "Weak entries").
pub const HOLDER_HOP_SHAPES: usize = HOLDER_HOPS + 3;
/// The site's bounded-record state.
pub const HOLDER_STATE: usize = crate::codegen_abi::PIC_HOLDER_STATE_WORD;
/// Legacy ABI bit: no new holder prime sets it.
#[cfg(test)]
const STATE_LATCHED: i64 = crate::codegen_abi::PIC_HOLDER_STATE_LATCHED;
const _: () = assert!(HOLDER_STATE == HOLDER_HOP_SHAPES + 1);

// Deep accessors use the collecting hit, with each intermediate ShapeId
// checked in the same site's bounded class-read record. Getter word 0 keeps
// the emitted direct-holder arm from answering without those checks.
// The emitted accessor arm (`perry-codegen/src/expr/property_get/
// accessor_arm.rs`) reads the pair and the getter from these words.
const _: () = assert!(HOLDER_HOPS == crate::codegen_abi::PIC_HOLDER_PAIR_WORD);
const _: () = assert!(HOLDER_HOP_SHAPES == crate::codegen_abi::PIC_HOLDER_GETTER_WORD);

/// A holder slot word with this bit set names the holder's SPILL position
/// (the low bits), not an inline slot: a key past the holder's inline region,
/// as `%Object.prototype%`'s `constructor` and keys added to a declared
/// class's prototype after it was built are. The holder's ShapeId pins the
/// key list, so the position names the key for as long as it matches, exactly
/// as it does for an inline slot. Spill positions stay far below this bit
/// (`spill::SPILL_MAX_FIELD_INDEX`). Inline words keep the bit clear, so the
/// common depth-1 inline hit stays one compare.
pub(super) const HOLDER_SLOT_SPILL: u32 = crate::codegen_abi::PIC_HOLDER_SLOT_SPILL_BIT as u32;
pub(crate) const HOLDER_MAX_DEPTH: usize = 4;

/// The value a holder slot word names on `addr` (see [`HOLDER_SLOT_SPILL`]),
/// or `None` when it must not be answered from the entry. An inline slot is
/// its word. A spill position answers only a value the generic getter would
/// take as found: `object::overflow_get` reads a spilled `undefined` (and a
/// never-written hole) as ABSENT and continues up the chain, so such a value
/// declines and the collecting path decides. Loads only: a GC leaf.
#[inline]
pub(super) unsafe fn holder_slot_value(addr: usize, slot: u32) -> Option<u64> {
    if slot & HOLDER_SLOT_SPILL == 0 {
        return Some(slot_bits(addr, slot));
    }
    holder_spill_value(addr, slot & !HOLDER_SLOT_SPILL)
}

#[inline]
unsafe fn holder_spill_value(addr: usize, index: u32) -> Option<u64> {
    crate::object::spill::spill_get_inline(addr, index as usize)
}

/// The slot word for key position `s` of the hop `addr` whose shape has
/// `inline` live inline slots: the inline slot, or the spill position when
/// the key's value lives in the object's spill storage now and the generic
/// getter takes it as found ([`holder_slot_value`]). `None` refuses the walk
/// (a legacy side-table overflow slot, spill disabled, or a value the getter
/// would read past).
unsafe fn holder_slot_word(
    addr: usize,
    keys: *const crate::ArrayHeader,
    position: u32,
    inline: u32,
) -> Option<u32> {
    let s = crate::object::key_attrs::key_storage_slot(keys, position);
    if s < inline {
        return Some(s);
    }
    if s as usize >= crate::object::spill::SPILL_MAX_FIELD_INDEX
        || crate::object::spill::spill_get(addr, s as usize).is_none()
    {
        return None;
    }
    Some(s | HOLDER_SLOT_SPILL)
}

#[inline]
unsafe fn slot_bits(addr: usize, slot: u32) -> u64 {
    std::ptr::read(
        (addr as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64,
    )
}

/// Does `name` belong to the read fast path at all? Index-like names live in
/// elements, and the refused names are synthesized or special-cased by the
/// getter.
fn holder_name_admitted(name: &[u8]) -> bool {
    !super::name_refused(name) && name != b"__proto__" && !name.iter().all(u8::is_ascii_digit)
}

/// The most intermediate hops a walk records: a class entry (`class_read`)
/// describes a deeper chain than the site's own holder entry has words for.
const WALK_HOPS: usize = 15;

/// An intermediate hop: its address and the ShapeId it had when proved.
type Hop = (usize, u32);

/// A walk's hops before it records any. A constant, so a new walk clears
/// them as one block rather than field by field around the padding.
const NO_HOPS: [Hop; WALK_HOPS] = [(0, 0); WALK_HOPS];

/// The answer the shapes give, found by a walk that allocates nothing.
struct Walk {
    holder: usize,
    holder_shape: u32,
    /// `None` = absent.
    slot: Option<u32>,
    hops: [Hop; WALK_HOPS],
    depth: usize,
}

/// A hop the entry may name: an ordinary, shaped, non-exotic object whose
/// ShapeId records the prototype identity it really has.
unsafe fn hop_admitted(addr: usize) -> bool {
    if !crate::value::addr_class::is_above_handle_band(addr)
        || !super::address_is_prime_stable(addr)
    {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    let obj = addr as *const ObjectHeader;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        || crate::object::overflow::has_overflow(obj)
    {
        return false;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return false;
    }
    true
}

/// What `obj` says its prototype identity is
/// ([`crate::object::shapes::object_proto_id`]), and the
/// recorded [[Prototype]] word that says it: one read of the word serves
/// both the identity check and the hop ([`next_from_word`]).
#[inline]
pub(super) unsafe fn stated_link(obj: *const ObjectHeader) -> (u64, u64) {
    let word = crate::object::shapes::object_prototype_word(obj);
    (crate::object::shapes::object_proto_id_for(obj, word), word)
}

/// [`next_prototype`] of `obj`, given its recorded word as [`stated_link`]
/// read it.
#[inline]
pub(crate) unsafe fn next_from_word(obj: *const ObjectHeader, word: u64) -> *const ObjectHeader {
    if word == 0 {
        return next_prototype(obj);
    }
    let p = crate::value::JSValue::from_bits(word);
    if p.is_pointer() {
        p.as_pointer()
    } else {
        std::ptr::null()
    }
}

/// Shape identities that pin the next object while the shape is unchanged.
/// MIXED carries the explicit prototype's serial. Bare CLASS still requires
/// a live identity-word compare; PER_OBJECT and UNIQUE are refused.
#[inline]
fn hop_identity_pins_link(pid: u64) -> bool {
    pid == PROTO_ID_DEFAULT
        || pid == PROTO_ID_NULL
        || pid < PROTO_ID_CLASS
        || (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid)
}

/// [`admitted_proto_id`], with `obj`'s recorded word.
pub(crate) unsafe fn admitted_link(obj: *const ObjectHeader) -> Option<(u64, u64)> {
    let record = crate::object::shapes::shape_record_by_id(object_shape_stamp(obj))?;
    let pid = record.proto_id();
    if !hop_identity_pins_link(pid) {
        // Only a declaration's immutable parent proof admits bare CLASS.
        // Reject other CLASS shapes before classifying their link: that
        // classification can consult synthetic/generic declaration metadata.
        let word = crate::object::shapes::object_prototype_word(obj);
        if crate::object::shapes::declaration_parent_identity(obj, word) != Some(pid) {
            return None;
        }
        // Preserve the read-semantics exclusions of object_proto_id_for.
        let meta = (*obj).meta;
        if (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
            || (!meta.is_null()
                && (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
        {
            return None;
        }
        return Some((pid, word));
    }
    // The prototype funnel minted this identity with the owner's keys and
    // link. Re-deriving it through object_proto_id_for repeats class/anon
    // registry queries at each hop and cannot strengthen the live shape
    // proof. A structural write moves the owner to a successor ShapeId.
    Some((pid, record.prototype_word()))
}

/// The prototype identity `obj`'s shape records, if it admits: a serial, the
/// default link, null or MIXED explicit link — and equal to what the object says it is.
pub(super) unsafe fn admitted_proto_id(obj: *const ObjectHeader) -> Option<u64> {
    admitted_link(obj).map(|(pid, _)| pid)
}

/// A MIXED identity records an explicit serial link. A bare CLASS identity
/// names its live identity word. Hits compare that word with the recorded
/// direct holder, then check each hop and holder ShapeId.
pub(crate) unsafe fn class_link(recv: *const ObjectHeader) -> Option<*const ObjectHeader> {
    let record = crate::object::shapes::shape_record_by_id(object_shape_stamp(recv))?;
    let pid = record.proto_id();
    // Most ordinary receivers have serial/default links. Their shape rules
    // out a class entry before any registry-backed identity derivation.
    if !(PROTO_ID_CLASS..PROTO_ID_UNIQUE).contains(&pid) {
        return None;
    }
    // The prototype funnel minted this identity with the receiver's link.
    // Re-deriving its generic origin through the registry cannot strengthen
    // the shape proof and would repeat a lock/hash lookup on every memo hit.
    let holder = if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
        next_from_word(recv, crate::object::shapes::object_prototype_word(recv))
    } else if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        {
            let bits = record.prototype_word();
            let value = crate::JSValue::from_bits(bits);
            if !value.is_pointer() {
                return None;
            }
            value.as_pointer::<ObjectHeader>()
        }
    } else {
        return None;
    };
    (!holder.is_null() && holder != recv).then_some(holder)
}

/// The [[Prototype]] a class instance with the recorded prototype `word`
/// (non-zero, read off the object) inherits from, when its ShapeId pins it:
/// `None` for the one its class implies (a CLASS identity), `Some(object)`
/// for a recorded one (a MIXED identity: a per-evaluation class's
/// prototype, or one set on the instance). `Err` when the shape does not
/// state that link.
///
/// A MIXED identity carries the prototype's serial, so two receivers of
/// one ShapeId inherit from one object; two evaluations of a class
/// declaration have two prototypes, hence two ShapeIds.
///
/// # Safety
/// `recv` is a live ordinary object; `word` is its recorded prototype word.
pub(crate) unsafe fn recorded_class_link(
    recv: *const ObjectHeader,
    word: u64,
) -> Result<Option<*const ObjectHeader>, ()> {
    let pid = shape_proto_id(object_shape_stamp(recv)).ok_or(())?;
    if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        if crate::object::shapes::declaration_parent_identity(recv, word) == Some(pid) {
            let parent = next_from_word(recv, word);
            return (!parent.is_null() && parent != recv)
                .then_some(Some(parent))
                .ok_or(());
        }
        return Ok(None);
    }
    if !(PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
        return Err(());
    }
    let holder = next_from_word(recv, word);
    if holder.is_null() || holder == recv {
        return Err(());
    }
    Ok(Some(holder))
}

pub(crate) struct HolderAccessor {
    pub(crate) hops: [(usize, u32); HOLDER_MAX_DEPTH - 1],
    pub(crate) depth: usize,
    pub(crate) holder: usize,
    pub(crate) shape: u32,
    /// The lane's holder slot word ([`HOLDER_SLOT_SPILL`] for a spill lane).
    pub(crate) slot: u32,
}

/// [`walk_to`] bounded by the site's own holder entry.
#[inline]
unsafe fn walk(recv: *const ObjectHeader, name: &[u8], class_first: bool) -> Option<Walk> {
    walk_to(recv, name, class_first, HOLDER_MAX_DEPTH)
}

/// The shapes' answer for `name` read off `recv`: at most `max_depth - 1`
/// intermediate hops that lack it, then the holder (or the terminal object
/// of an absent read). A longer chain is `None`.
unsafe fn walk_to(
    recv: *const ObjectHeader,
    name: &[u8],
    class_first: bool,
    max_depth: usize,
) -> Option<Walk> {
    keyed::walk_to_key(
        recv,
        keyed::KeyRef::Name {
            word: 0,
            bytes: name,
        },
        class_first,
        max_depth,
    )
}

/// What the receiver's shapes prove about a computed-key read of one name
/// (#10753); see [`dynamic_own_or_absent`].
pub(crate) enum DynamicKeyVerdict {
    /// The name is an own DATA key of the receiver at `slot`: an inline slot
    /// when `slot < live`, an overflow slot otherwise.
    Own { slot: u32, live: u32 },
    /// An ordinary Get of the name reaches no property. The payload is the
    /// ShapeId of `%Object.prototype%` the proof rests on, or 0 when the
    /// receiver's shape links to null and nothing past it is consulted.
    Absent(u32),
}

/// The shape facts behind a computed-key read (#10753), from ONE lookup of the
/// name in the receiver's own key list: the own data slot that holds it, or
/// the proof that an ordinary Get reaches no property.
///
/// Both verdicts rest on the facts a depth-1 ABSENT holder entry is primed
/// from: an ordinary receiver ([`ordinary_receiver`]: not a dictionary, no
/// indexed elements, no exotic read semantics, a live ShapeId), an admitted
/// name (no index-like or synthesized key) and no accessor for the name on the
/// receiver. Found among the receiver's own keys, the key's slot IS the answer:
/// an own data property shadows everything behind it. Not found, the read is
/// absent only when a [`walk`] ends at `%Object.prototype%` with no hop
/// between. The caller files an absent verdict
/// (`object::read_stub::read_stub_prime_absent`) only after the generic Get
/// has answered `undefined`, as `prime_read_holder` files its entries only
/// after the getter agreed.
///
/// Allocation-free and never calls user code.
///
/// # Safety
/// `obj` is a plausible object address; `name` stays valid for the call.
pub(crate) unsafe fn dynamic_own_or_absent(
    obj: *const ObjectHeader,
    name: &[u8],
) -> Option<DynamicKeyVerdict> {
    if !holder_name_admitted(name) {
        return None;
    }
    let recv = ordinary_receiver(obj as usize)?;
    // The verdicts below read the layout alone. A receiver with an overflow
    // holds own keys the layout does not list, and holes where it deleted
    // listed ones (object model step 4): the complete read decides.
    if crate::object::overflow::has_overflow(recv) {
        return None;
    }
    let shape = object_shape_descriptor(recv)?;
    if shape.object_kind == crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias {
        return None;
    }
    if !shape.object_kind.is_ordinary_layout() {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null() {
        if let Some(slot) =
            crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
        {
            if crate::object::key_attrs::key_is_accessor_at(keys, slot) {
                return None;
            }
            return Some(DynamicKeyVerdict::Own {
                slot,
                live: shape.live_inline_slot_count,
            });
        }
    }
    if admitted_proto_id(recv)? == PROTO_ID_NULL {
        return Some(DynamicKeyVerdict::Absent(0));
    }
    let w = walk(recv, name, false)?;
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    (w.slot.is_none() && w.depth == 1 && object_prototype != 0 && w.holder == object_prototype)
        .then_some(DynamicKeyVerdict::Absent(w.holder_shape))
}

/// [`dynamic_own_or_absent`]'s ABSENT verdict alone: `Some(terminal)` when
/// `obj`'s shapes prove that an ordinary Get of `name` reaches no property.
///
/// # Safety
/// As [`dynamic_own_or_absent`].
#[cfg(test)]
pub(crate) unsafe fn dynamic_absent_terminal(obj: *const ObjectHeader, name: &[u8]) -> Option<u32> {
    match dynamic_own_or_absent(obj, name)? {
        DynamicKeyVerdict::Absent(terminal) => Some(terminal),
        DynamicKeyVerdict::Own { .. } => None,
    }
}
