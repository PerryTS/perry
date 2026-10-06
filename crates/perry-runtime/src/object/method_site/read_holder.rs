//! The read site's HOLDER entry: `o.k` where `k` is not an own key of the
//! receiver, answered by facts of two shapes.
//!
//! * The receiver's ShapeId `S` vouches that `k` is not own, that the receiver
//!   is an ordinary object, and its [[Prototype]] identity. Only a serial
//!   identity or `PROTO_ID_DEFAULT` (the realm's `Object.prototype`) pins ONE
//!   object for the GC-leaf data/absent path. A collecting accessor entry may
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
//! The entry lives in the read site's own cache (`PicCache` words
//! [`HOLDER_RECV`]..=[`HOLDER_REGISTERED`]). The holder and the hops are
//! STRONG roots, rewritten when they move ([`scan_read_holder_roots_mut`]).
//!
//! # Emitted form
//!
//! Emitted code holds nothing of the entry. The read site's ShapeId compare
//! misses, and its one GC-leaf front call
//! (`field_get_set::ic_miss::read_confirm::js_object_get_field_ic_front`)
//! asks [`entry_answer`] after the ways, the spill entry and a latched site's
//! confirm, so an own-key read pays nothing for it. A decline is `TAG_HOLE`
//! and the site continues to the collecting slow call. Once a worker starts,
//! the front declines before reading any holder-entry word: the entry belongs
//! to the primary heap and no agent's GC scans it after the gate.
//!
//! # Priming
//!
//! Only from the primary agent's read miss handler, which already knows the key
//! is not own. Data and absent entries are recorded only after the generic
//! getter's answer agrees with the shapes. An accessor is published only
//! after its pair is validated, then its getter runs once. A worker
//! agent's start gates all further
//! holder hits and primes; stale entries stop being roots and can collect.
//!
//! A miss whose receiver the live entry already answers is served from the
//! entry and primes nothing (a caller that does not emit the check, such as
//! the class-field read's miss arm, reaches here on every read). A site that
//! refused once, or whose entry was replaced for a different receiver shape
//! [`MAX_REPRIMES`] times, is LATCHED in its own state word
//! ([`HOLDER_STATE`]): it never walks or primes again, and its misses take the
//! path they took before the entry existed.

use super::{next_prototype, ordinary_receiver, WORKER_AGENTS_EXIST};
use crate::object::shapes::{
    object_shape_descriptor, object_shape_stamp, shape_proto_id, PIC_ID_TOKEN_BIT, PROTO_ID_CLASS,
    PROTO_ID_DEFAULT, PROTO_ID_MIXED, PROTO_ID_NULL, PROTO_ID_UNIQUE,
};
use crate::object::{ObjectHeader, PicCache, PicCacheSlot};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) mod class_read;
#[cfg(any(test, feature = "regex-engine"))]
pub(crate) mod probe;

/// The receiver's ShapeId as a PIC token (`ShapeId | PIC_ID_TOKEN_BIT`), or 0
/// for an empty entry. A zeroed cache is therefore an empty one: no token is 0.
pub const HOLDER_RECV: usize = crate::codegen_abi::PIC_HOLDER_RECV_WORD;
/// The holder's (or, for an absent entry, the terminal object's) address.
pub const HOLDER_OBJ: usize = crate::codegen_abi::PIC_HOLDER_OBJ_WORD;
/// Low 32 bits: the holder's ShapeId. High 32 bits: the third hop's ShapeId.
pub const HOLDER_SHAPE: usize = crate::codegen_abi::PIC_HOLDER_SHAPE_WORD;
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
/// from the pair at each hit; 0 for a setter-only pair and for a lane in the
/// holder's spill storage, which the emitted arm cannot load (the slow call
/// derives the getter from the pair). A code address, never a GC pointer:
/// the root scan never visits this word.
/// The accessor's pair itself (its raw address) is in [`HOLDER_HOPS`], a
/// strong root rewritten on move like the hop words it replaces.
pub const HOLDER_HOP_SHAPES: usize = HOLDER_HOPS + 3;
/// The site's holder state: [`STATE_REGISTERED`], [`STATE_LATCHED`] and the
/// count of re-primes for a different receiver shape.
pub const HOLDER_STATE: usize = crate::codegen_abi::PIC_HOLDER_STATE_WORD;
/// The cache is on the root list.
const STATE_REGISTERED: i64 = 1;
/// The site refused, or is polymorphic in its non-own receivers: no walk and
/// no prime from here on.
const STATE_LATCHED: i64 = crate::codegen_abi::PIC_HOLDER_STATE_LATCHED;
const STATE_REPRIME_SHIFT: u32 = 8;
const _: () = assert!(HOLDER_STATE == HOLDER_HOP_SHAPES + 1);
/// Re-primes for a different receiver shape a site takes before it latches.
const MAX_REPRIMES: i64 = 4;

pub const HOLDER_ABSENT_DEPTH1: i64 = crate::codegen_abi::PIC_HOLDER_ABSENT_DEPTH1;
pub const HOLDER_STUB: u64 = 1 << 63;
const HOLDER_ABSENT_BIT: u64 = 1 << 62;
/// A direct-prototype accessor. It can collect and therefore never answers
/// from the GC-leaf front call: the emitted arm calls the getter, and the
/// collecting slow call asks [`try_cached_accessor`] first.
const HOLDER_ACCESSOR: u64 = crate::codegen_abi::PIC_HOLDER_ACCESSOR_BIT as u64;
// Deep accessors use the collecting hit, with each intermediate ShapeId
// checked in the same site's bounded class-read record. Getter word 0 keeps
// the emitted direct-holder arm from answering without those checks.
const HOLDER_ACCESSOR_DEEP: u64 = HOLDER_STUB;
// The emitted accessor arm (`perry-codegen/src/expr/property_get/
// accessor_arm.rs`) reads the pair and the getter from these words.
const _: () = assert!(HOLDER_HOPS == crate::codegen_abi::PIC_HOLDER_PAIR_WORD);
const _: () = assert!(HOLDER_HOP_SHAPES == crate::codegen_abi::PIC_HOLDER_GETTER_WORD);
/// Depth-1 ABSENT entries can share one terminal holder across several
/// receiver shapes. The spare hop words hold ShapeIds, never GC pointers.
const HOLDER_MULTI_ABSENT: u64 = 1 << 60;
const MULTI_ABSENT_EXTRA_IDS: usize = 9;
const MULTI_ABSENT_NEXT_MASK: u64 = 0xf;
/// A multi-shape DATA entry keeps its holder slot word above the next-way
/// index.
const MULTI_SLOT_SHIFT: u32 = 8;

/// The holder slot word of a multi-shape data entry's kind.
#[inline]
fn multi_slot(kind: u64) -> u32 {
    (kind >> MULTI_SLOT_SHIFT) as u32
}
const HOLDER_DEPTH_SHIFT: u32 = 32;
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

/// Every cache that holds (or held) a holder entry, for the primary agent's
/// root scan until a worker starts. The entries are in the per-site caches;
/// this is only where the scan finds them.
static HOLDER_SITES: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

per_test_global! {
    static PRIMES_HOLDER: AtomicU64 = AtomicU64::new(0);
    static PRIMES_ABSENT: AtomicU64 = AtomicU64::new(0);
    static REFUSED_HOLDER: AtomicU64 = AtomicU64::new(0);
    static PRIMES_ACCESSOR: AtomicU64 = AtomicU64::new(0);
    static HITS_ACCESSOR: AtomicU64 = AtomicU64::new(0);
    static HOLDER_REWRITES: AtomicU64 = AtomicU64::new(0);
    static ACCESSOR_REWRITES: AtomicU64 = AtomicU64::new(0);
    static SAME_SHAPE_RELINKS: AtomicU64 = AtomicU64::new(0);
    static CLASS_PRIMES: AtomicU64 = AtomicU64::new(0);
}

/// `(data primes, absent primes, refusals)`.
pub fn read_holder_stats() -> (u64, u64, u64) {
    (
        PRIMES_HOLDER.load(Ordering::Relaxed),
        PRIMES_ABSENT.load(Ordering::Relaxed),
        REFUSED_HOLDER.load(Ordering::Relaxed),
    )
}

pub fn read_accessor_stats() -> (u64, u64) {
    (
        PRIMES_ACCESSOR.load(Ordering::Relaxed),
        HITS_ACCESSOR.load(Ordering::Relaxed),
    )
}

pub fn class_read_stats() -> (u64, u64, u64) {
    class_read::stats()
}

pub fn read_holder_rewrites() -> u64 {
    HOLDER_REWRITES.load(Ordering::Relaxed)
}

pub fn read_accessor_rewrites() -> u64 {
    ACCESSOR_REWRITES.load(Ordering::Relaxed)
}

pub fn read_accessor_same_shape_relinks() -> u64 {
    SAME_SHAPE_RELINKS.load(Ordering::Relaxed)
}

pub fn read_accessor_class_primes() -> u64 {
    CLASS_PRIMES.load(Ordering::Relaxed)
}

#[inline]
fn refuse() {
    REFUSED_HOLDER.fetch_add(1, Ordering::Relaxed);
}

/// Refuse, and latch `cache` (when the site has one) so it never walks again.
#[inline]
unsafe fn refuse_and_latch(cache: *mut PicCache) {
    refuse();
    if !cache.is_null() {
        (*cache)[HOLDER_STATE] |= STATE_LATCHED;
    }
}

/// The entry's answer (value bits) for a receiver whose PIC token is
/// `token`, or `None` when the entry is empty, names another receiver shape,
/// or any hop or holder word it recorded has moved. Reads site words and
/// object words only: a GC leaf.
///
/// The loaded slot needs no `TAG_HOLE` test, for the reason the emitted MRU
/// hit needs none: every delete is a shape transition (#10826), so a holder
/// whose ShapeId still matches has not had the slot cleared.
#[inline(always)]
pub(crate) unsafe fn entry_answer(c: &PicCache, token: i64) -> Option<u64> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    if token == 0 {
        return None;
    }
    // The common data/stub entry checks its receiver token before decoding
    // kinds. Only a token miss can search the extra ABSENT shapes; keeping
    // that search off the ordinary inherited hit avoids taxing every read.
    if c[HOLDER_RECV] != token {
        // Only a multi-shape entry has further receiver tokens; every other
        // site (a class entry's site among them) leaves on this load.
        if c[HOLDER_KIND] as u64 & HOLDER_MULTI_ABSENT == 0 {
            return None;
        }
        return multi_absent_extra_answer(c, token);
    }
    let kind = c[HOLDER_KIND];
    // A depth-1 data holder is the common inherited-read hit. Its kind is
    // exactly an inline slot number; answer it before the absent, accessor,
    // multi-shape and deeper-hop decoding below.
    if (kind as u64) < u64::from(HOLDER_SLOT_SPILL) {
        let holder = c[HOLDER_OBJ] as usize;
        if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
            return None;
        }
        return Some(slot_bits(holder, kind as u32));
    }
    entry_answer_other(c, kind)
}

/// Uncommon entries keep their complete depth/absence validation off the
/// inlined depth-1 data hit, including the class-field read's caller.
#[cold]
#[inline(never)]
unsafe fn entry_answer_other(c: &PicCache, kind: i64) -> Option<u64> {
    if kind as u64 & (HOLDER_ACCESSOR | HOLDER_MULTI_ABSENT) != 0 {
        if kind as u64 & HOLDER_ACCESSOR != 0 {
            return None;
        }
        return multi_answer(c, kind as u64);
    }
    let (depth, absent, slot) = if kind >= 0 {
        (1, kind == HOLDER_ABSENT_DEPTH1, kind as u32)
    } else {
        let k = kind as u64;
        (
            ((k >> HOLDER_DEPTH_SHIFT) & 0xF) as usize,
            k & HOLDER_ABSENT_BIT != 0,
            k as u32,
        )
    };
    let hop_shapes = c[HOLDER_HOP_SHAPES] as u64;
    let shape_words = [
        hop_shapes as u32,
        (hop_shapes >> 32) as u32,
        (c[HOLDER_SHAPE] as u64 >> 32) as u32,
    ];
    for i in 0..depth.saturating_sub(1).min(HOLDER_MAX_DEPTH - 1) {
        if shape_word(c[HOLDER_HOPS + i] as usize) != shape_words[i] {
            return None;
        }
    }
    let holder = c[HOLDER_OBJ] as usize;
    if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
        return None;
    }
    if absent {
        return Some(crate::value::TAG_UNDEFINED);
    }
    holder_slot_value(holder, slot)
}

/// A second through tenth receiver shape of a multi-shape depth-1 entry is a
/// rare path relative to one-token data hits. It shares the holder (the
/// terminal object for an absent entry) but must still prove the entry is
/// live and the holder's ShapeId has not changed.
#[cold]
#[inline(never)]
unsafe fn multi_absent_extra_answer(c: &PicCache, token: i64) -> Option<u64> {
    let kind = c[HOLDER_KIND] as u64;
    if c[HOLDER_RECV] == 0
        || kind & HOLDER_MULTI_ABSENT == 0
        || kind & HOLDER_ACCESSOR != 0
        || !(0..MULTI_ABSENT_EXTRA_IDS).any(|i| multi_absent_id(c, i) == token as u32)
    {
        return None;
    }
    multi_answer(c, kind)
}

/// A multi-shape depth-1 entry's answer once a receiver token matched: the
/// holder's ShapeId, then `undefined` or the holder's slot. Every receiver
/// shape it names was admitted on its own walk and getter confirmation, with
/// this holder and holder ShapeId and (for data) this slot word.
#[inline]
unsafe fn multi_answer(c: &PicCache, kind: u64) -> Option<u64> {
    let holder = c[HOLDER_OBJ] as usize;
    if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
        return None;
    }
    if kind & HOLDER_ABSENT_BIT != 0 {
        return Some(crate::value::TAG_UNDEFINED);
    }
    holder_slot_value(holder, multi_slot(kind))
}

/// Spare depth-1 ABSENT words: the upper half of the holder-shape word and
/// four words that otherwise hold intermediate hop addresses/shapes.
/// The root scanner visits only HOLDER_OBJ for this entry kind.
#[inline]
fn multi_absent_id(c: &PicCache, i: usize) -> u32 {
    debug_assert!(i < MULTI_ABSENT_EXTRA_IDS);
    if i == 0 {
        (c[HOLDER_SHAPE] as u64 >> 32) as u32
    } else {
        let word = HOLDER_HOPS + (i - 1) / 2;
        (c[word] as u64 >> (32 * ((i - 1) % 2))) as u32
    }
}

#[inline]
fn set_multi_absent_id(c: &mut PicCache, i: usize, id: u32) {
    debug_assert!(i < MULTI_ABSENT_EXTRA_IDS);
    if i == 0 {
        c[HOLDER_SHAPE] =
            ((c[HOLDER_SHAPE] as u64 & u64::from(u32::MAX)) | (u64::from(id) << 32)) as i64;
    } else {
        let word = HOLDER_HOPS + (i - 1) / 2;
        let shift = 32 * ((i - 1) % 2);
        let mask = u64::from(u32::MAX) << shift;
        c[word] = ((c[word] as u64 & !mask) | (u64::from(id) << shift)) as i64;
    }
}

/// The site's DECLARED-CLASS entry (`class_read`) for `recv`, from the GC-leaf
/// read front, asked after [`entry_answer`] declined: a class instance's
/// inherited data or absent key, proved by the receiver's ShapeId and class
/// id, the class lookup-surface generation (its direct link) and the hop and
/// holder ShapeIds. Loads and compares only.
///
/// # Safety
/// `c` is a live site cache; `recv` an object whose ShapeId `token` carries.
#[inline]
pub(crate) unsafe fn class_entry_answer(
    c: &PicCache,
    recv: *const ObjectHeader,
    token: i64,
) -> Option<u64> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 || token == 0 {
        return None;
    }
    class_read::leaf_answer(c, recv, token)
}

/// The site's holder entry asked for `handle`, without priming: what the
/// emitted tower's holder check answers, for a runtime caller that asks the
/// site's words itself (`typed_feedback::guards`' class-field miss arm).
///
/// # Safety
/// `handle` is a pointer above the handle band; `cache_slot` null or the
/// site's live read cache slot.
#[inline(always)]
pub(crate) unsafe fn read_holder_hit(
    handle: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if cache.is_null() {
        return None;
    }
    let stamp = object_shape_stamp(handle);
    if stamp == 0 {
        return None;
    }
    entry_answer(&*cache, (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64).map(f64::from_bits)
}

#[inline]
unsafe fn shape_word(addr: usize) -> u32 {
    object_shape_stamp(addr as *const ObjectHeader)
}

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
    crate::object::spill::spill_get(addr, index as usize)
}

/// The slot word for key position `s` of the hop `addr` whose shape has
/// `inline` live inline slots: the inline slot, or the spill position when
/// the key's value lives in the object's spill storage now and the generic
/// getter takes it as found ([`holder_slot_value`]). `None` refuses the walk
/// (a legacy side-table overflow slot, spill disabled, or a value the getter
/// would read past).
unsafe fn holder_slot_word(addr: usize, s: u32, inline: u32) -> Option<u32> {
    if s < inline {
        return Some(s);
    }
    if s as usize >= crate::object::spill::SPILL_MAX_FIELD_INDEX
        || !crate::object::spill::object_spill_enabled()
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

/// A declared-class instance whose class's prototype object does not exist
/// yet: materialize it, as any `C.prototype` read does, so the walk below has
/// the object the class's prototype members (and `C.prototype.k = v` values,
/// which `js_register_prototype_method` records before the object exists) are
/// read from. Without it the site walks the class registry by name on every
/// read.
///
/// The materialization allocates with collection SUPPRESSED: the miss
/// handler holds the receiver and key raw and takes the generic getter with
/// them when the prime declines, so nothing may move here. It installs only
/// the class's own declared members; no user code runs.
///
/// # Safety
/// `obj` is the miss handler's object receiver.
unsafe fn materialize_class_prototype(obj: *const ObjectHeader) {
    if ordinary_receiver(obj as usize).is_none() {
        return;
    }
    let class_id = (*obj).class_id;
    let is_bare_class = shape_proto_id(object_shape_stamp(obj))
        .is_some_and(|pid| (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid));
    if !is_bare_class || !crate::object::class_decl_prototype_object(class_id).is_null() {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::class_registry::class_decl_prototype_value(class_id);
}

/// Could a [[Get]] of `name` on `obj` run an accessor? The read-side form of
/// `method_site::key_may_be_accessor`: a data property's attributes
/// (non-enumerable, read-only, non-configurable: the meta record's
/// `attr_key_bits`) do not change what a Get answers, so only the accessor
/// Bloom bit short-cuts, and the authoritative descriptor state decides the
/// rest. `%Object.prototype%.constructor` and every class prototype's methods
/// are non-enumerable data properties.
unsafe fn key_may_be_accessor(obj: *const ObjectHeader, name: &[u8]) -> bool {
    let meta = (*obj).meta;
    if !meta.is_null() {
        let bit = 1u64 << (crate::object::key_bytes_hash(name.as_ptr(), name.len()) & 63);
        if (*meta).accessor_key_bits & bit != 0 {
            return true;
        }
    }
    if crate::object::descriptor_state::object_has_descriptors(obj as usize) {
        let Ok(name) = std::str::from_utf8(name) else {
            return true;
        };
        if crate::object::descriptor_state::get_accessor_descriptor(obj as usize, name).is_some() {
            return true;
        }
    }
    false
}

/// [`holder_name_admitted`] for an ordinary read site's receiver. The
/// method-site name refusals cover `constructor` because a CLASS instance's
/// `constructor` is synthesized from the class registry (`instance_constructor_value`)
/// rather than read off its chain. Default and ordinary serial links have
/// no declared class prototype to synthesize from: their constructor is the
/// chain's data slot like any other key.
unsafe fn read_name_admitted(recv: *const ObjectHeader, name: &[u8]) -> bool {
    if name == b"constructor" {
        return admitted_proto_id(recv).is_some_and(|id| id < PROTO_ID_CLASS);
    }
    holder_name_admitted(name)
}

/// The most intermediate hops a walk records: a class entry (`class_read`)
/// describes a deeper chain than the site's own holder entry has words for.
const WALK_HOPS: usize = class_read::CLASS_READ_MAX_DEPTH - 1;

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
    /// An accessor entry's getter word (see [`HOLDER_HOP_SHAPES`]); 0 for
    /// data/absence. Its pair is `hops[0].0`.
    getter: usize,
}

/// A hop the entry may name: an ordinary, shaped, non-exotic object whose
/// ShapeId records the prototype identity it really has.
unsafe fn hop_admitted(addr: usize, name: &[u8]) -> bool {
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
        || crate::closure::is_closure_ptr(addr)
        || crate::object::dictionary::is_dictionary(obj)
    {
        return false;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
    {
        return false;
    }
    !key_may_be_accessor(obj, name)
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
pub(super) unsafe fn next_from_word(obj: *const ObjectHeader, word: u64) -> *const ObjectHeader {
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

/// [`admitted_proto_id`], with `obj`'s recorded word.
unsafe fn admitted_link(obj: *const ObjectHeader) -> Option<(u64, u64)> {
    let pid = shape_proto_id(object_shape_stamp(obj))?;
    let serial = pid != PROTO_ID_DEFAULT && pid < crate::object::shapes::PROTO_ID_CLASS;
    if !(serial || pid == PROTO_ID_DEFAULT || pid == PROTO_ID_NULL) {
        return None;
    }
    let (stated, word) = stated_link(obj);
    (stated == pid).then_some((pid, word))
}

/// The prototype identity `obj`'s shape records, if it admits: a serial, the
/// default link or null — and equal to what the object says it is.
pub(super) unsafe fn admitted_proto_id(obj: *const ObjectHeader) -> Option<u64> {
    admitted_link(obj).map(|(pid, _)| pid)
}

/// A MIXED identity records an explicit serial link. A bare CLASS identity
/// names its registry-resolved prototype: priming resolves the live
/// declared-prototype pointer, and a later relink retires that pointer's
/// ShapeId (see the module docs), which the hit's holder compare sees.
pub(super) unsafe fn class_link(recv: *const ObjectHeader) -> Option<*const ObjectHeader> {
    let pid = shape_proto_id(object_shape_stamp(recv))?;
    let (stated, word) = stated_link(recv);
    if stated != pid {
        return None;
    }
    let holder = if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
        next_from_word(recv, word)
    } else if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        crate::object::class_decl_prototype_object((*recv).class_id)
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
    if crate::object::shapes::object_proto_id_for(recv, word) != pid {
        return Err(());
    }
    if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
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
    /// The pair's raw address: the value the holder's lane holds.
    pub(crate) pair: usize,
    /// How the hit calls the getter ([`HOLDER_HOP_SHAPES`]).
    getter: usize,
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

/// Find the first property on a shape-pinned chain. An accessor answers;
/// data shadows it. Every intermediate shape proves absence and its link.
pub(crate) unsafe fn accessor_walk(
    recv: *const ObjectHeader,
    name: &[u8],
) -> Option<HolderAccessor> {
    if !holder_name_admitted(name)
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return None;
    }
    let (mut holder, _) = accessor_holder(recv)?;
    let mut hops = [(0, 0); HOLDER_MAX_DEPTH - 1];
    for depth in 1..=HOLDER_MAX_DEPTH {
        let addr = holder as usize;
        if !crate::value::addr_class::is_above_handle_band(addr)
            || !super::address_is_prime_stable(addr)
        {
            return None;
        }
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            || crate::object::dictionary::is_dictionary(holder)
        {
            return None;
        }
        let meta = (*holder).meta;
        if !meta.is_null()
            && ((*meta).elements != 0
                || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
        {
            return None;
        }
        let shape = object_shape_descriptor(holder)?;
        if !shape.object_kind.is_ordinary_layout() {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        let candidate = (shape.summary & crate::object::key_attrs::SUMMARY_ACCESSOR != 0)
            .then(|| {
                crate::object::key_attrs::keys_find_accessor_slot_resolved(
                    keys,
                    shape.logical_key_count,
                    name,
                )
            })
            .flatten();
        if let Some(slot) = candidate {
            // No getter ran while walking. Only a positive accessor needs
            // name lookups in the nearer holders to prove it is unshadowed.
            for &(hop, _) in &hops[..depth - 1] {
                let nearer = object_shape_descriptor(hop as *const ObjectHeader)?;
                if crate::object::keys_find_slot_by_bytes_resolved(
                    nearer.keys as usize as *const crate::array::ArrayHeader,
                    nearer.logical_key_count,
                    name,
                )
                .is_some()
                {
                    return None;
                }
            }
            let slot = holder_slot_word(addr, slot, shape.live_inline_slot_count)?;
            let lane = holder_slot_value(addr, slot)?;
            let getter = crate::object::accessor_pair::site_getter_word_of_value(lane)?;
            return Some(HolderAccessor {
                hops,
                depth,
                holder: addr,
                shape: object_shape_stamp(holder),
                slot,
                pair: (lane & crate::value::POINTER_MASK) as usize,
                getter,
            });
        }
        if depth == HOLDER_MAX_DEPTH {
            return None;
        }
        // Declared prototypes also carry MIXED identities: the class word
        // plus the serial of the explicitly recorded parent. That serial
        // pins one next holder, just like a plain object's recorded link.
        let pid = shape_proto_id(object_shape_stamp(holder))?;
        let (pid, word) = if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
            let (stated, word) = stated_link(holder);
            if stated != pid {
                return None;
            }
            (pid, word)
        } else {
            admitted_link(holder)?
        };
        if pid == PROTO_ID_NULL {
            return None;
        }
        hops[depth - 1] = (addr, object_shape_stamp(holder));
        let next = if pid == PROTO_ID_DEFAULT {
            crate::array::object_prototype_addr_if_resolved() as *const ObjectHeader
        } else {
            next_from_word(holder, word)
        };
        if next.is_null() || next == holder || next == recv {
            return None;
        }
        holder = next;
    }
    None
}

/// Call the getter an accessor entry names with `recv` as `this`: a compiled
/// class getter through its code address, a function object (read from
/// `pair`, the primed pair the caller has just compared with the lane)
/// through the closure-getter entry. The receiver is the call's argument and
/// nothing reads it afterwards, so nothing is rooted here: the getter's own
/// frame roots its receiver.
#[inline]
unsafe fn invoke_getter(
    recv: *const ObjectHeader,
    getter: usize,
    pair: usize,
) -> crate::value::JSValue {
    let this = crate::value::js_nanbox_pointer(recv as i64);
    if getter == crate::object::accessor_pair::holder_closure_getter_entry() {
        let result = crate::object::accessor_pair::holder_closure_getter(this, pair as i64);
        return crate::value::JSValue::from_bits(result.to_bits());
    }
    if getter == 0 {
        return crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    let f = crate::closure::body_call::js_method_body_fn!(getter as *const u8;);
    crate::value::JSValue::from_bits(f(this).to_bits())
}

/// Collecting-path class data/absence memo; the leaf front never consults it.
pub(crate) unsafe fn try_cached_class_read(
    recv: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    class_read::try_hit(recv, cache_slot)
}

/// Collecting read-miss arm. The GC-leaf front always declines this kind.
///
/// Every fact is a shape fact or the lane's own value:
/// * the receiver's ShapeId (the token) proves the key is not own and names
///   the receiver's prototype identity, hence the holder (a serial, or a
///   bare class whose registry link retires the holder's ShapeId if it ever
///   changes);
/// * the holder's ShapeId proves the slot is still an accessor lane;
/// * the lane's value is the primed pair, so the getter it names is the one
///   `[[Get]]` would call.
///
/// The getter is called with the ORIGINAL receiver as `this`.
#[inline]
pub(crate) unsafe fn try_cached_accessor(
    recv: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if cache.is_null() {
        return None;
    }
    let c = &*cache;
    if c[HOLDER_KIND] as u64 & HOLDER_ACCESSOR == 0 {
        return None;
    }
    accessor_entry_hit(recv, c)
}

/// [`try_cached_accessor`] past its kind test. Out of line: every collecting
/// read miss on an object asks the entry first, and a site without an
/// accessor entry must pay only the inlined kind test.
/// Validate the accessor lane without invoking its getter.
#[inline]
unsafe fn accessor_entry_pair(recv: *const ObjectHeader, c: &PicCache) -> Option<usize> {
    let kind = c[HOLDER_KIND] as u64;
    if kind & HOLDER_ACCESSOR == 0 {
        return None;
    }
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    let stamp = object_shape_stamp(recv);
    if stamp == 0 || c[HOLDER_RECV] != (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64 {
        return None;
    }
    if kind & HOLDER_ACCESSOR_DEEP != 0 && !class_read::accessor_hops_match(c) {
        return None;
    }
    let holder = c[HOLDER_OBJ] as usize;
    let lane = crate::value::POINTER_TAG | c[HOLDER_HOPS] as u64;
    if shape_word(holder) != c[HOLDER_SHAPE] as u32
        || holder_slot_value(holder, kind as u32) != Some(lane)
    {
        return None;
    }
    Some(c[HOLDER_HOPS] as usize)
}

#[inline(never)]
unsafe fn accessor_entry_hit(
    recv: *const ObjectHeader,
    c: &PicCache,
) -> Option<crate::value::JSValue> {
    let pair = accessor_entry_pair(recv, c)?;
    let kind = c[HOLDER_KIND] as u64;
    let lane = crate::value::POINTER_TAG | pair as u64;
    // A spill lane's entry keeps getter word 0 for the emitted arm; the pair
    // (immutable, and just compared) names its getter.
    let getter = if kind & HOLDER_ACCESSOR_DEEP != 0 || kind as u32 & HOLDER_SLOT_SPILL != 0 {
        crate::object::accessor_pair::site_getter_word_of_value(lane)?
    } else {
        c[HOLDER_HOP_SHAPES] as usize
    };
    HITS_ACCESSOR.fetch_add(1, Ordering::Relaxed);
    Some(invoke_getter(recv, getter, c[HOLDER_HOPS] as usize))
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
    debug_assert!(max_depth <= WALK_HOPS + 1);
    let mut w = Walk {
        holder: 0,
        holder_shape: 0,
        slot: None,
        hops: NO_HOPS,
        depth: 0,
        getter: 0,
    };
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    let mut current = recv;
    for depth in 1..=max_depth {
        // `%Object.prototype%` is an immutable-prototype exotic object: its
        // [[Prototype]] is null for its whole life, whatever its shape's
        // identity word says, so reaching it ends the chain.
        let terminal = depth > 1 && current as usize == object_prototype;
        // `current`'s recorded word, once read for its identity check.
        let mut word = None;
        let pid = if terminal {
            PROTO_ID_NULL
        } else if depth == 1 && class_first {
            // A bare CLASS ShapeId does not pin the registry's live
            // C.prototype. The collecting class-read hit compares that
            // pointer on every use; this walk records it as the first hop.
            crate::object::shapes::PROTO_ID_CLASS
        } else {
            let mixed = if class_first {
                let pid = shape_proto_id(object_shape_stamp(current))?;
                if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
                    let (stated, w) = stated_link(current);
                    word = Some(w);
                    (stated == pid).then_some(pid)
                } else {
                    None
                }
            } else {
                None
            };
            match mixed {
                Some(pid) => pid,
                None => {
                    let (pid, w) = admitted_link(current)?;
                    word = Some(w);
                    pid
                }
            }
        };
        if pid == PROTO_ID_NULL {
            // `current` is the terminal object, and it lacks `name`.
            if depth == 1 {
                return None;
            }
            let (h, sh) = w.hops[depth - 2];
            w.hops[depth - 2] = (0, 0);
            w.holder = h;
            w.holder_shape = sh;
            w.depth = depth - 1;
            return Some(w);
        }
        let next = if depth == 1 && class_first {
            class_link(recv)?
        } else if pid == PROTO_ID_DEFAULT {
            object_prototype as *const ObjectHeader
        } else {
            match word {
                Some(w) => next_from_word(current, w),
                None => next_prototype(current),
            }
        };
        if next.is_null() || next == current || next == recv || !hop_admitted(next as usize, name) {
            return None;
        }
        let shape = object_shape_descriptor(next)?;
        if !shape.object_kind.is_ordinary_layout() || object_shape_stamp(next) == 0 {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(s) =
                crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
            {
                let s = holder_slot_word(next as usize, s, shape.live_inline_slot_count)?;
                w.holder = next as usize;
                w.holder_shape = object_shape_stamp(next);
                w.slot = Some(s);
                w.depth = depth;
                return Some(w);
            }
        }
        if depth == max_depth {
            // One more object would be needed: either the holder or the null
            // link past the last hop.
            if next as usize != object_prototype && admitted_proto_id(next) != Some(PROTO_ID_NULL) {
                return None;
            }
            w.holder = next as usize;
            w.holder_shape = object_shape_stamp(next);
            w.depth = depth;
            return Some(w);
        }
        w.hops[depth - 1] = (next as usize, object_shape_stamp(next));
        current = next;
    }
    None
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
    if key_may_be_accessor(recv, name) {
        return None;
    }
    let shape = object_shape_descriptor(recv)?;
    if !shape.object_kind.is_ordinary_layout() {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null() {
        if let Some(slot) =
            crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
        {
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

/// The walk for a FUNCTION receiver (#10497): `f.k` on a plain function on
/// its base shape, whose ShapeId (`closure::shape`) pins that its own keys are
/// exactly the intrinsic `name`, `length` and `prototype` and that its
/// [[Prototype]] is the realm's `%Function.prototype%` (any own-key install,
/// delete, descriptor or `setPrototypeOf` moves it to the FunctionDictionary
/// shape). The chain from `%Function.prototype%` on is an ordinary [`walk`],
/// with `%Function.prototype%` as the entry's first hop. The intrinsic names
/// and the function methods the getter synthesizes are refused, so only a
/// key that genuinely lives on (or is absent from) the prototype objects is
/// described. Allocation-free.
unsafe fn function_walk(closure: usize, name: &[u8]) -> Option<Walk> {
    if !holder_name_admitted(name)
        || matches!(
            name,
            b"name"
                | b"length"
                | b"prototype"
                | b"caller"
                | b"arguments"
                | b"call"
                | b"apply"
                | b"bind"
        )
        || std::str::from_utf8(name)
            .ok()
            .is_none_or(|n| crate::object::reified_function_method_name(n).is_some())
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(closure)?;
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || !super::address_is_prime_stable(closure)
    {
        return None;
    }
    let c = closure as *const crate::closure::ClosureHeader;
    if !crate::closure::shape::closure_on_base_shape(c)
        || shape_proto_id((*c).shape_id)? != crate::closure::shape::INTRINSIC_SERIAL_FUNCTION
    {
        return None;
    }
    let fp = crate::array::function_prototype_addr_if_resolved();
    if fp == 0 || !hop_admitted(fp, name) {
        return None;
    }
    let fp_obj = fp as *const ObjectHeader;
    let shape = object_shape_descriptor(fp_obj)?;
    let fp_shape = object_shape_stamp(fp_obj);
    if !shape.object_kind.is_ordinary_layout() || fp_shape == 0 {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if !keys.is_null() {
        if let Some(s) =
            crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)
        {
            return Some(Walk {
                holder: fp,
                holder_shape: fp_shape,
                slot: Some(holder_slot_word(fp, s, shape.live_inline_slot_count)?),
                hops: NO_HOPS,
                getter: 0,
                depth: 1,
            });
        }
    }
    // Absent on `%Function.prototype%`: the rest of the chain, one hop deeper.
    let mut inner = walk(fp_obj, name, false)?;
    if inner.depth >= HOLDER_MAX_DEPTH {
        return None;
    }
    inner.depth += 1;
    inner.hops.copy_within(0..HOLDER_MAX_DEPTH - 2, 1);
    inner.hops[0] = (fp, fp_shape);
    Some(inner)
}

/// [`prime_read_holder`] for a function receiver (`f.k`, #10497): the site's
/// holder entry, keyed by the function's base ShapeId, for a key
/// [`function_walk`] describes. Returns the answer when it ran the read here
/// (the getter, then the confirmation and the publish); `None`, having
/// allocated nothing, when the caller's own path must answer.
///
/// # Safety
/// `closure` is the miss handler's receiver address, `key` its key.
pub(crate) unsafe fn prime_function_read(
    closure: usize,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<f64> {
    if cache_slot.is_null()
        || key.is_null()
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return None;
    }
    let existing = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    let stamp = object_shape_stamp(closure as *const ObjectHeader);
    if stamp == 0 {
        return None;
    }
    let token = (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64;
    if !existing.is_null() {
        if let Some(bits) = entry_answer(&*existing, token) {
            return Some(f64::from_bits(bits));
        }
        if (*existing)[HOLDER_STATE] & STATE_LATCHED != 0 {
            return None;
        }
    }
    let name = crate::string::header_str_checked(key)?.as_bytes();
    function_walk(closure, name)?;
    // The answer, from the path the miss handler takes for a function
    // receiver. It can run user code and collect: root across it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(closure as *mut ObjectHeader);
    let key_handle = scope.root_string_ptr(key);
    let (value, closure) = handle.across_mut::<ObjectHeader, _>(|| {
        crate::object::field_get_set::closure_dynamic_prop_by_key(closure, key).unwrap_or_else(
            || {
                f64::from_bits(
                    crate::object::field_get_set::get_field_by_name_after_site_miss(
                        closure as *const ObjectHeader,
                        key,
                    )
                    .bits(),
                )
            },
        )
    });
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return Some(value);
    }
    let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
    key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
        let Some(name) = crate::string::header_str_checked(key).map(str::as_bytes) else {
            return Some(value);
        };
        let Some(w) = function_walk(closure as usize, name) else {
            refuse_and_latch(cache);
            return Some(value);
        };
        let bits = value.to_bits();
        let confirmed = match w.slot {
            None => bits == crate::value::TAG_UNDEFINED,
            Some(s) => {
                holder_slot_value(w.holder, s) == Some(bits) && bits != crate::value::TAG_HOLE
            }
        };
        if !confirmed {
            refuse_and_latch(cache);
        } else if !cache.is_null() {
            publish(cache, closure, &w, false);
        }
        Some(value)
    })
}

/// Prime `cache_slot`'s holder entry for `obj.key`, whose key the caller has
/// proved is not own. Returns the answer (from the generic getter) when the
/// receiver took the generic read here; `None` when it did not, and the caller
/// reads as before.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` receiver; `key` a live string header.
pub(crate) unsafe fn prime_read_holder(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    if cache_slot.is_null()
        || key.is_null()
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return None;
    }
    // A receiver the live entry answers is served from it: nothing to prime.
    // A latched site keeps the caller's path for its holder entry. The latch
    // is that entry's: a site latched by one receiver's refusal still primes
    // its class entries (`class_read`), which have ways and a latch of their
    // own, for the declared-class receivers it reads.
    let existing = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    let mut latched = false;
    if !existing.is_null() {
        let stamp = object_shape_stamp(obj);
        if stamp != 0 {
            let token = (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64;
            if let Some(bits) = entry_answer(&*existing, token) {
                return Some(crate::value::JSValue::from_bits(bits));
            }
        }
        if (*existing)[HOLDER_STATE] & STATE_LATCHED != 0 {
            if !class_read::may_prime(&*existing) {
                return None;
            }
            latched = true;
        }
    }
    materialize_class_prototype(obj);
    let name = crate::string::header_str_checked(key)?.as_bytes();
    let recv = ordinary_receiver(obj as usize)?;
    // The class lane already proves data or absence through holder shapes.
    // Let it answer before doing a second walk looking for an accessor;
    // its pre-walk declines accessor lanes without running their getters.
    if let Some(value) = class_read::prime(recv, key, cache_slot, name) {
        return Some(value);
    }
    // A latched holder site keeps accessor priming disabled.
    let acc = if latched {
        None
    } else {
        accessor_walk(recv, name)
    };
    if let Some(acc) = acc {
        let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
        if !cache.is_null() {
            let mut hops = NO_HOPS;
            hops[0].0 = acc.pair;
            let w = Walk {
                holder: acc.holder,
                holder_shape: acc.shape,
                slot: Some(acc.slot),
                hops,
                depth: 1,
                getter: acc.getter,
            };
            publish(cache, recv, &w, true);
            if acc.depth > 1 {
                class_read::publish_accessor_hops(cache, &acc.hops, acc.depth - 1);
                (*cache)[HOLDER_KIND] |= HOLDER_ACCESSOR_DEEP as i64;
                (*cache)[HOLDER_HOP_SHAPES] = 0;
            }
        }
        return Some(invoke_getter(recv, acc.getter, acc.pair));
    }
    if latched {
        return None;
    }
    // Cheap pre-walk: a receiver the entry could never describe keeps the
    // caller's path and pays nothing for the getter below. A site with no
    // cache yet stays without one and uses the generic getter.
    //
    // A walk that ends at the default link needs `%Object.prototype%`, which
    // is materialized lazily: while it is unresolved the walk cannot pin it,
    // and refusing here would leave the site on the generic path (the getter
    // below is what resolves it). So an unresolved realm
    // does not decide the pre-walk; the walk after the getter does.
    let realm_pending = crate::array::object_prototype_addr_if_resolved() == 0;
    if !read_name_admitted(recv, name)
        || key_may_be_accessor(recv, name)
        || (walk(recv, name, false).is_none() && !realm_pending)
    {
        refuse_and_latch(existing);
        return None;
    }

    // The answer, from the generic getter. It can run user code and collect,
    // so the receiver is rooted across it and everything is re-read after.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj as *mut ObjectHeader);
    let key_handle = scope.root_string_ptr(key);
    let (value, obj) = handle.across_mut::<ObjectHeader, _>(|| {
        crate::object::field_get_set::get_field_by_name_after_site_miss(obj, key)
    });
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return Some(value);
    }
    // From here a refusal has already run the getter, so the site latches:
    // the next miss must not walk and run it again only to refuse again.
    let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
    key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
        let name = crate::string::header_str_checked(key)?.as_bytes();
        let Some(recv) = ordinary_receiver(obj as usize) else {
            refuse_and_latch(cache);
            return Some(value);
        };
        let Some(w) = walk(recv, name, false) else {
            refuse_and_latch(cache);
            return Some(value);
        };
        // This scoped key pointer is used only by the non-collecting walk
        // and confirmation. The generic getter has already returned.
        let bits = value.bits();
        let confirmed = match w.slot {
            None => bits == crate::value::TAG_UNDEFINED,
            Some(s) => {
                holder_slot_value(w.holder, s) == Some(bits) && bits != crate::value::TAG_HOLE
            }
        };
        if !confirmed {
            refuse_and_latch(cache);
            return Some(value);
        }
        if !cache.is_null() {
            publish(cache, recv, &w, false);
        }
        Some(value)
    })
}

unsafe fn publish(cache: *mut PicCache, recv: *const ObjectHeader, w: &Walk, accessor: bool) {
    let c = &mut *cache;
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
    // A default-prototype optional field is often absent from many receiver
    // shapes at ONE read site (TypeScript AST's `.name` is the canonical
    // case). Keep the already-confirmed receiver shapes in this site's spare
    // words when they all share the same terminal object and terminal shape.
    // Every new token is admitted only after the caller's generic getter
    // returned `undefined` and `walk` proved the complete chain absent.
    //
    // The same holds for an inherited DATA key read off several receiver
    // shapes that all reach it on the same holder (`o.constructor` on plain
    // objects of many shapes): the holder, its ShapeId and the slot word are
    // shared, only the receiver token differs.
    let old_kind = c[HOLDER_KIND] as u64;
    if accessor
        && old_kind & HOLDER_ACCESSOR != 0
        && c[HOLDER_RECV] != 0
        && c[HOLDER_RECV] != token
        && crate::object::shapes::shape_record_by_id(c[HOLDER_RECV] as u32).is_none()
    {
        // A retired receiver shape cannot compete with this live one. Its
        // replacement is cache expiry, not continuing polymorphic churn.
        // Reprime through the same receiver/holder/lane checks below.
        c[HOLDER_STATE] &= ((1 << STATE_REPRIME_SHIFT) - 1) & !STATE_LATCHED;
    }
    let old_multi =
        old_kind & (HOLDER_MULTI_ABSENT | HOLDER_ACCESSOR | HOLDER_STUB) == HOLDER_MULTI_ABSENT;
    let same_answer = match w.slot {
        None => {
            old_kind == HOLDER_ABSENT_DEPTH1 as u64
                || (old_multi && old_kind & HOLDER_ABSENT_BIT != 0)
        }
        Some(s) => {
            old_kind == u64::from(s)
                || (old_multi && old_kind & HOLDER_ABSENT_BIT == 0 && multi_slot(old_kind) == s)
        }
    };
    if !accessor
        && w.depth == 1
        && same_answer
        && c[HOLDER_RECV] != 0
        && c[HOLDER_RECV] != token
        && c[HOLDER_OBJ] as usize == w.holder
        && c[HOLDER_SHAPE] as u32 == w.holder_shape
    {
        let next = if old_kind & HOLDER_MULTI_ABSENT == 0 {
            0
        } else {
            (old_kind & MULTI_ABSENT_NEXT_MASK) as usize
        };
        debug_assert!(next < MULTI_ABSENT_EXTRA_IDS);
        let previous = c[HOLDER_RECV] as u32;
        c[HOLDER_RECV] = 0;
        set_multi_absent_id(c, next, previous);
        let answer = match w.slot {
            None => HOLDER_ABSENT_BIT,
            Some(s) => u64::from(s) << MULTI_SLOT_SHIFT,
        };
        c[HOLDER_KIND] =
            (answer | HOLDER_MULTI_ABSENT | ((next + 1) % MULTI_ABSENT_EXTRA_IDS) as u64) as i64;
        c[HOLDER_RECV] = token;
        if w.slot.is_some() {
            PRIMES_HOLDER.fetch_add(1, Ordering::Relaxed);
        } else {
            PRIMES_ABSENT.fetch_add(1, Ordering::Relaxed);
        }
        super::stats_report_enabled();
        return;
    }
    if c[HOLDER_RECV] != 0 && c[HOLDER_RECV] != token {
        // The site's non-own receivers take more than one shape. One entry
        // cannot hold them; after a few replacements the site stops priming.
        let n = (c[HOLDER_STATE] >> STATE_REPRIME_SHIFT) + 1;
        c[HOLDER_STATE] = (c[HOLDER_STATE] & ((1 << STATE_REPRIME_SHIFT) - 1))
            | (n << STATE_REPRIME_SHIFT)
            | if n >= MAX_REPRIMES { STATE_LATCHED } else { 0 };
        if n >= MAX_REPRIMES {
            refuse();
            return;
        }
    }
    if accessor
        && c[HOLDER_RECV] != 0
        && c[HOLDER_KIND] as u64 & HOLDER_ACCESSOR != 0
        && c[HOLDER_OBJ] as usize != w.holder
        && c[HOLDER_SHAPE] as u32 == w.holder_shape
    {
        SAME_SHAPE_RELINKS.fetch_add(1, Ordering::Relaxed);
    }
    class_read::clear_accessor_hops(c);
    c[HOLDER_RECV] = 0;
    c[HOLDER_OBJ] = w.holder as i64;
    c[HOLDER_SHAPE] = (u64::from(w.holder_shape) | u64::from(w.hops[2].1) << 32) as i64;
    c[HOLDER_KIND] = if accessor {
        (HOLDER_ACCESSOR | u64::from(w.slot.expect("accessor has slot"))) as i64
    } else {
        match (w.depth, w.slot) {
            (1, Some(s)) => i64::from(s),
            (1, None) => HOLDER_ABSENT_DEPTH1,
            (d, s) => {
                (HOLDER_STUB
                    | if s.is_none() { HOLDER_ABSENT_BIT } else { 0 }
                    | (d as u64) << HOLDER_DEPTH_SHIFT
                    | u64::from(s.unwrap_or(0))) as i64
            }
        }
    };
    for i in 0..HOLDER_MAX_DEPTH - 1 {
        c[HOLDER_HOPS + i] = w.hops[i].0 as i64;
    }
    // Accessor pairs are immutable. The second hop word is unused by an
    // accessor (deep hops have class_read storage), so retain its native
    // getter code alongside the pair. Replacing the lane invalidates both.
    #[cfg(any(test, feature = "regex-engine"))]
    if accessor {
        let pair = w.hops[0].0 as *const crate::array::ArrayHeader;
        let words = crate::array::array_elements_ptr(pair);
        c[HOLDER_HOPS + 1] =
            probe::getter_code(*words.add(crate::object::accessor_pair::PAIR_GET)) as i64;
    }
    c[HOLDER_HOP_SHAPES] = if accessor {
        // The emitted arm loads inline lanes only (see `HOLDER_HOP_SHAPES`).
        if w.slot.is_some_and(|s| s & HOLDER_SLOT_SPILL != 0) {
            0
        } else {
            w.getter as i64
        }
    } else {
        (u64::from(w.hops[0].1) | u64::from(w.hops[1].1) << 32) as i64
    };
    if c[HOLDER_STATE] & STATE_REGISTERED == 0 {
        c[HOLDER_STATE] |= STATE_REGISTERED;
        if let Ok(mut sites) = HOLDER_SITES.lock() {
            sites.push(cache as usize);
        }
    }
    // Last: the entry is live only once every other word is written.
    c[HOLDER_RECV] = token;
    if accessor {
        PRIMES_ACCESSOR.fetch_add(1, Ordering::Relaxed);
        if shape_proto_id(object_shape_stamp(recv))
            .is_some_and(|pid| (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid))
        {
            CLASS_PRIMES.fetch_add(1, Ordering::Relaxed);
        }
    } else if w.slot.is_some() {
        PRIMES_HOLDER.fetch_add(1, Ordering::Relaxed);
    } else {
        PRIMES_ABSENT.fetch_add(1, Ordering::Relaxed);
    }
    super::stats_report_enabled();
}

/// Root scan: live entries are primary roots only until a worker starts. Once
/// the sticky gate is set, `entry_answer` declines before touching any entry
/// word, and no agent needs to retain the old primary objects.
pub(crate) fn scan_read_holder_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
    {
        return;
    }
    let Ok(sites) = HOLDER_SITES.lock() else {
        return;
    };
    for &site in sites.iter() {
        // SAFETY: registered caches are PIC-arena allocations
        // (`pic_arena_alloc`), which are never freed.
        let c = unsafe { &mut *(site as *mut PicCache) };
        if c[HOLDER_RECV] != 0 {
            if visitor.visit_i64_slot(&mut c[HOLDER_OBJ]) {
                HOLDER_REWRITES.fetch_add(1, Ordering::Relaxed);
                if c[HOLDER_KIND] as u64 & HOLDER_ACCESSOR != 0 {
                    ACCESSOR_REWRITES.fetch_add(1, Ordering::Relaxed);
                }
            }
            if c[HOLDER_KIND] as u64 & HOLDER_MULTI_ABSENT == 0 {
                // Accessors retain only their pair here. The next word is
                // native code, not a managed pointer; deep hops are separate.
                let count = if c[HOLDER_KIND] as u64 & HOLDER_ACCESSOR != 0 {
                    1
                } else {
                    HOLDER_MAX_DEPTH - 1
                };
                for i in 0..count {
                    visitor.visit_i64_slot(&mut c[HOLDER_HOPS + i]);
                }
            }
        }
        class_read::scan_roots(c, visitor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn getter_two(_this: f64) -> f64 {
        2.0
    }
    extern "C" fn getter_eight(_this: f64) -> f64 {
        8.0
    }

    /// The entry an accessor prime publishes for `acc`.
    fn accessor_entry(acc: &HolderAccessor) -> Walk {
        let mut hops = NO_HOPS;
        hops[0].0 = acc.pair;
        Walk {
            holder: acc.holder,
            holder_shape: acc.shape,
            slot: Some(acc.slot),
            hops,
            depth: 1,
            getter: acc.getter,
        }
    }

    #[test]
    fn inherited_accessor_rechecks_intermediate_shapes() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "inherited_accessor_rechecks_intermediate_shapes",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let holder = crate::object::js_object_alloc(0, 0);
            let middle = crate::object::js_object_alloc(0x0C3C_79A6, 0);
            let receiver = crate::object::js_object_alloc(0, 0);
            let value = |p| crate::value::js_nanbox_pointer(p as i64);
            crate::object::js_object_set_prototype_of(
                value(holder),
                f64::from_bits(crate::value::TAG_NULL),
            );
            crate::object::js_object_set_prototype_of(value(middle), value(holder));
            crate::object::js_object_set_prototype_of(value(receiver), value(middle));
            assert!(
                shape_proto_id(object_shape_stamp(middle))
                    .is_some_and(|pid| (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid)),
                "the intermediate class prototype must exercise a MIXED link"
            );
            let install = |obj: *mut ObjectHeader, raw_get| {
                crate::object::set_builtin_accessor_pair(
                    obj as usize,
                    "path".to_string(),
                    crate::object::accessor_pair::Accessor {
                        raw_get,
                        ..Default::default()
                    },
                    crate::object::PropertyAttrs::new(true, false, true),
                );
            };
            install(holder, getter_two as *const () as usize);
            let key = crate::string::js_string_from_bytes(b"path".as_ptr(), 4);
            let mut slot = std::ptr::null_mut();
            assert_eq!(
                prime_read_holder(receiver, key, &mut slot).map(|v| v.as_number()),
                Some(2.0)
            );
            assert!(!slot.is_null(), "the inherited accessor must prime a site");
            assert_ne!((*slot)[HOLDER_KIND] as u64 & HOLDER_ACCESSOR_DEEP, 0);
            assert_eq!(
                try_cached_accessor(receiver, &mut slot).map(|v| v.as_number()),
                Some(2.0)
            );
            install(middle, getter_eight as *const () as usize);
            assert!(
                try_cached_accessor(receiver, &mut slot).is_none(),
                "a nearer accessor invalidates the deep entry"
            );
            assert_eq!(
                prime_read_holder(receiver, key, &mut slot).map(|v| v.as_number()),
                Some(8.0)
            );
            assert_eq!(
                try_cached_accessor(receiver, &mut slot).map(|v| v.as_number()),
                Some(8.0)
            );
        }
    }

    /// Two holders with exactly one ShapeId but different compiled getters.
    /// Replacing a declared class's registry pointer leaves the receiver's
    /// bare CLASS ShapeId unchanged. The replacement must retire the old
    /// holder's ShapeId, so the hit's holder compare refuses the stale entry
    /// with no global word to consult.
    #[test]
    fn class_accessor_rechecks_same_shape_holder_link() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "class_accessor_rechecks_same_shape_holder_link",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_79A3;
        let scope = crate::gc::RuntimeHandleScope::new();
        let p1 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        let p2 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        for (holder, raw_get) in [
            (&p1, getter_two as *const () as usize),
            (&p2, getter_eight as *const () as usize),
        ] {
            holder.with_mut_ptr::<ObjectHeader, _>(|ptr| {
                crate::object::set_builtin_accessor_pair(
                    ptr as usize,
                    "path".to_owned(),
                    crate::object::accessor_pair::Accessor {
                        raw_get,
                        ..Default::default()
                    },
                    crate::object::PropertyAttrs::new(true, false, true),
                );
            });
        }
        p1.with_const_ptr::<ObjectHeader, _>(|first| {
            p2.with_const_ptr::<ObjectHeader, _>(|second| {
                assert_ne!(first, second);
                assert_eq!(unsafe { object_shape_stamp(first) }, unsafe {
                    object_shape_stamp(second)
                });
            });
        });

        let packed = b"holder_class_key";
        let keys = crate::object::js_build_class_keys_array(
            CID,
            1,
            packed.as_ptr(),
            packed.len() as u32,
            0,
        );
        let recv_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
            keys as usize as u64,
            1,
            CID,
            0,
        );
        let recv = crate::object::js_object_alloc_class_inline_keys_stamped(
            CID, 0, 1, keys, recv_shape, 0,
        );
        let recv = scope.root_raw_mut_ptr(recv);
        recv.with_const_ptr::<ObjectHeader, _>(|receiver| {
            assert_eq!(
                unsafe { shape_proto_id(object_shape_stamp(receiver)) },
                Some(PROTO_ID_CLASS | u64::from(CID))
            );

            p1.with_const_ptr::<ObjectHeader, _>(|ptr| {
                crate::object::test_seed_class_decl_prototype_object_root(CID, ptr as usize);
            });
            let first = unsafe { accessor_walk(receiver, b"path") }.expect("first accessor");
            let cache: &'static mut PicCache =
                Box::leak(Box::new([0; crate::codegen_abi::PIC_CACHE_WORDS]));
            let mut slot: PicCacheSlot = cache;
            unsafe { publish(cache, receiver, &accessor_entry(&first), true) };
            assert_eq!(
                cache[HOLDER_HOP_SHAPES] as usize,
                getter_two as *const () as usize
            );
            assert_eq!(cache[HOLDER_HOPS] as usize, first.pair);
            assert_eq!(
                unsafe { try_cached_accessor(receiver, &mut slot) }.map(|v| v.as_number()),
                Some(2.0)
            );

            let old_relinks = read_accessor_same_shape_relinks();
            p2.with_const_ptr::<ObjectHeader, _>(|ptr| {
                crate::object::test_seed_class_decl_prototype_object_root(CID, ptr as usize);
            });
            assert_eq!(unsafe { object_shape_stamp(receiver) }, recv_shape);
            // The relink is seen through the old holder's ShapeId alone.
            p1.with_const_ptr::<ObjectHeader, _>(|old| {
                assert_ne!(unsafe { object_shape_stamp(old) }, first.shape);
                assert_ne!(unsafe { object_shape_stamp(old) }, 0);
            });
            assert!(
                unsafe { try_cached_accessor(receiver, &mut slot) }.is_none(),
                "stale getter was served after registry replacement"
            );
            let second = unsafe { accessor_walk(receiver, b"path") }.expect("second accessor");
            unsafe { publish(cache, receiver, &accessor_entry(&second), true) };
            assert!(read_accessor_same_shape_relinks() > old_relinks);
            assert_eq!(
                unsafe { try_cached_accessor(receiver, &mut slot) }.map(|v| v.as_number()),
                Some(8.0)
            );
            // A getter replacement may retain a compiled setter. Neither a
            // new prime nor an existing hit may mistake it for setter-only.
            // Keep the deliberate same-shape slot replacement noncollecting.
            let _no_gc = crate::gc::GcSuppressScope::new();
            extern "C" fn closure_getter(
                _closure: *const crate::closure::ClosureHeader,
                _this: crate::closure::JsThis,
            ) -> f64 {
                9.0
            }
            let closure = crate::closure::js_closure_alloc(crate::fn_info!(closure_getter, 0), 0);
            let pair = unsafe {
                crate::object::accessor_pair::pair_new(crate::object::accessor_pair::Accessor {
                    get: crate::value::js_nanbox_pointer(closure as i64).to_bits(),
                    raw_set: getter_eight as *const () as usize,
                    ..Default::default()
                })
            };
            p2.with_mut_ptr::<ObjectHeader, _>(|holder| unsafe {
                crate::object::slot_store::store_object_field_slot(
                    holder,
                    second.slot as usize,
                    crate::value::js_nanbox_pointer(pair as i64).to_bits(),
                );
                assert_eq!(object_shape_stamp(holder), second.shape);
            });
            // The replaced lane misses the primed pair, with no ShapeId
            // transition to see it by.
            assert!(unsafe { try_cached_accessor(receiver, &mut slot) }.is_none());
            // A function-object getter re-primes with the closure-ABI word
            // and is called with the receiver as `this`.
            let third = unsafe { accessor_walk(receiver, b"path") }.expect("closure accessor");
            assert_eq!(
                third.getter,
                crate::object::accessor_pair::holder_closure_getter_entry()
            );
            assert_eq!(third.pair, pair as usize);
            unsafe { publish(cache, receiver, &accessor_entry(&third), true) };
            assert_eq!(
                unsafe { try_cached_accessor(receiver, &mut slot) }.map(|v| v.as_number()),
                Some(9.0)
            );
        });
    }

    #[test]
    fn ten_receiver_shapes_share_one_confirmed_absent_terminal() {
        if !crate::object::method_site::run_with_fresh_worker_gate(
            "ten_receiver_shapes_share_one_confirmed_absent_terminal",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let base = crate::object::shapes::SHAPE_ID_BASE;
        let holder = Box::new(ObjectHeader {
            class_id: 0,
            parent_class_id: base + 100,
            meta: std::ptr::null_mut(),
        });
        let other = Box::new(ObjectHeader {
            class_id: 0,
            parent_class_id: base + 101,
            meta: std::ptr::null_mut(),
        });
        let mut cache = [0; crate::object::PIC_CACHE_WORDS];
        // The stack cache is not a process-lifetime PIC allocation. Skip
        // registration; this test exercises only the published words.
        cache[HOLDER_STATE] = STATE_REGISTERED;
        let mut recv = ObjectHeader {
            class_id: 0,
            parent_class_id: base,
            meta: std::ptr::null_mut(),
        };
        let absent = Walk {
            holder: (&*holder as *const ObjectHeader) as usize,
            holder_shape: base + 100,
            slot: None,
            hops: NO_HOPS,
            depth: 1,
            getter: 0,
        };
        for i in 0..11 {
            recv.parent_class_id = base + i;
            unsafe { publish(&mut cache, &recv, &absent, false) };
        }
        assert_ne!(cache[HOLDER_KIND] as u64 & HOLDER_MULTI_ABSENT, 0);
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base)) as i64) },
            None,
            "the oldest of eleven shapes must leave a ten-shape site"
        );
        for i in 1..11 {
            let token = (PIC_ID_TOKEN_BIT | u64::from(base + i)) as i64;
            assert_eq!(
                unsafe { entry_answer(&cache, token) },
                Some(crate::value::TAG_UNDEFINED)
            );
        }
        // A new shape after an own-key shadow has no entry, while a terminal
        // mutation invalidates every receiver shape in the shared entry.
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64) },
            None
        );
        let mut moved = Box::new(ObjectHeader {
            class_id: 0,
            parent_class_id: base + 100,
            meta: std::ptr::null_mut(),
        });
        cache[HOLDER_OBJ] = (&mut *moved as *mut ObjectHeader) as i64;
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64) },
            Some(crate::value::TAG_UNDEFINED)
        );
        moved.parent_class_id = base + 102;
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64) },
            None
        );
        // A different terminal never inherits the old entry's receiver set.
        let distinct = Walk {
            holder: (&*other as *const ObjectHeader) as usize,
            holder_shape: base + 101,
            ..absent
        };
        recv.parent_class_id = base + 11;
        unsafe { publish(&mut cache, &recv, &distinct, false) };
        assert_eq!(cache[HOLDER_KIND], HOLDER_ABSENT_DEPTH1);
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64) },
            None
        );
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64) },
            Some(crate::value::TAG_UNDEFINED)
        );
        WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
        assert_eq!(
            unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64) },
            None
        );
    }

    /// A class instance has a valid, stamped ShapeId, but its prototype is
    /// resolved through the class vtable. The holder walk must refuse it even
    /// when the shape and the object's current prototype id agree.
    #[test]
    fn class_prototype_identity_is_refused_by_read_holder() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_79A2;
        let packed = b"holder_class_key";
        let keys = crate::object::js_build_class_keys_array(
            CID,
            1,
            packed.as_ptr(),
            packed.len() as u32,
            0,
        );
        let shape_id = crate::object::shapes::js_object_shape_id_for_class_keys(
            keys as usize as u64,
            1,
            CID,
            0,
        );
        let obj =
            crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 1, keys, shape_id, 0);
        let claimed = shape_proto_id(shape_id).expect("class shape must be stamped");
        assert_eq!(claimed, crate::object::shapes::class_proto_id(CID));
        assert_eq!(
            unsafe { crate::object::shapes::object_proto_id(obj) },
            claimed
        );
        assert!(claimed >= crate::object::shapes::PROTO_ID_CLASS);
        assert_eq!(unsafe { admitted_proto_id(obj) }, None);
    }
}
