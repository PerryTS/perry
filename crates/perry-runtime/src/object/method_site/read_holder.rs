//! The read site's HOLDER entry: `o.k` where `k` is not an own key of the
//! receiver, answered by facts of two shapes.
//!
//! * The receiver's ShapeId `S` vouches that `k` is not own, that the receiver
//!   is an ordinary object, and its [[Prototype]] identity. Only a serial
//!   identity or `PROTO_ID_DEFAULT` (the realm's `Object.prototype`) pins ONE
//!   object for the GC-leaf data/absent path. A collecting accessor entry may
//!   also use a declared class identity: a registry generation check proves
//!   its rooted holder is still the registered prototype on every hit.
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
//! collecting class-accessor route additionally checks the class registry's
//! lookup-surface generation for a bare declared-prototype link.
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
//! getter's answer agrees with the shapes. A class accessor is published only
//! after its compiled pair is validated, then its getter runs once. A worker
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

use super::{key_may_be_accessor, next_prototype, ordinary_receiver, WORKER_AGENTS_EXIST};
use crate::object::shapes::{
    object_proto_id, object_shape_descriptor, object_shape_stamp, shape_proto_id, PIC_ID_TOKEN_BIT,
    PROTO_ID_CLASS, PROTO_ID_DEFAULT, PROTO_ID_MIXED, PROTO_ID_NULL, PROTO_ID_UNIQUE,
};
use crate::object::{ObjectHeader, PicCache, PicCacheSlot};
use std::sync::atomic::{AtomicU64, Ordering};

mod class_read;

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
/// | `0 ..= u32::MAX` | depth 1, the value is the holder's inline slot |
/// | [`HOLDER_ABSENT_DEPTH1`] | depth 1, absent: the answer is `undefined` |
/// | [`HOLDER_ACCESSOR`] + slot | direct class-prototype accessor; collecting hit only |
/// | [`HOLDER_MULTI_ABSENT`] | depth-1 absent for up to ten receiver shapes |
/// | negative | [`HOLDER_STUB`] set: depth 2..=4 and/or a deep absent entry |
pub const HOLDER_KIND: usize = crate::codegen_abi::PIC_HOLDER_KIND_WORD;
/// First of three intermediate hop addresses (depth 2..=4).
pub const HOLDER_HOPS: usize = HOLDER_KIND + 1;
/// Data/absence: first and second hop ShapeIds. Class accessor: the class
/// lookup-surface generation at prime time (no hop pointer uses this word).
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
/// A direct class-prototype accessor. It can collect and therefore never
/// answers from the GC-leaf front call.
const HOLDER_ACCESSOR: u64 = 1 << 61;
/// Depth-1 ABSENT entries can share one terminal holder across several
/// receiver shapes. The spare hop words hold ShapeIds, never GC pointers.
const HOLDER_MULTI_ABSENT: u64 = 1 << 60;
const MULTI_ABSENT_EXTRA_IDS: usize = 9;
const MULTI_ABSENT_NEXT_MASK: u64 = 0xf;
const HOLDER_DEPTH_SHIFT: u32 = 32;
const HOLDER_MAX_DEPTH: usize = 4;

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
    entry_answer_after_worker_gate(c, token)
}

/// The same site-word validation for primary-agent unit tests whose process
/// may already have started a worker. The public entry remains gate guarded.
#[inline(always)]
unsafe fn entry_answer_after_worker_gate(c: &PicCache, token: i64) -> Option<u64> {
    if token == 0 {
        return None;
    }
    // The common data/stub entry checks its receiver token before decoding
    // kinds. Only a token miss can search the extra ABSENT shapes; keeping
    // that search off the ordinary inherited hit avoids taxing every read.
    if c[HOLDER_RECV] != token {
        return multi_absent_extra_answer(c, token);
    }
    let kind = c[HOLDER_KIND];
    // A depth-1 data holder is the common inherited-read hit. Its kind is
    // exactly an inline slot number; answer it before the absent, accessor,
    // multi-shape and deeper-hop decoding below.
    if (kind as u64) <= u32::MAX as u64 {
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
        if kind as u64 & HOLDER_ACCESSOR != 0 || kind as u64 & HOLDER_ABSENT_BIT == 0 {
            return None;
        }
        return (shape_word(c[HOLDER_OBJ] as usize) == c[HOLDER_SHAPE] as u32)
            .then_some(crate::value::TAG_UNDEFINED);
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
    Some(slot_bits(holder, slot))
}

/// A second through tenth ABSENT receiver shape is a rare path relative to
/// one-token data hits. It shares the terminal holder but must still prove the
/// entry is live and its ShapeId has not changed.
#[cold]
#[inline(never)]
unsafe fn multi_absent_extra_answer(c: &PicCache, token: i64) -> Option<u64> {
    let kind = c[HOLDER_KIND] as u64;
    if c[HOLDER_RECV] == 0
        || kind & (HOLDER_MULTI_ABSENT | HOLDER_ABSENT_BIT)
            != HOLDER_MULTI_ABSENT | HOLDER_ABSENT_BIT
        || kind & HOLDER_ACCESSOR != 0
        || !(0..MULTI_ABSENT_EXTRA_IDS).any(|i| multi_absent_id(c, i) == token as u32)
    {
        return None;
    }
    (shape_word(c[HOLDER_OBJ] as usize) == c[HOLDER_SHAPE] as u32)
        .then_some(crate::value::TAG_UNDEFINED)
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

/// The answer the shapes give, found by a walk that allocates nothing.
struct Walk {
    holder: usize,
    holder_shape: u32,
    /// `None` = absent.
    slot: Option<u32>,
    hops: [(usize, u32); HOLDER_MAX_DEPTH - 1],
    depth: usize,
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

/// The prototype identity `obj`'s shape records, if it admits: a serial, the
/// default link or null — and equal to what the object says it is.
pub(super) unsafe fn admitted_proto_id(obj: *const ObjectHeader) -> Option<u64> {
    let pid = shape_proto_id(object_shape_stamp(obj))?;
    let serial = pid != PROTO_ID_DEFAULT && pid < crate::object::shapes::PROTO_ID_CLASS;
    if !(serial || pid == PROTO_ID_DEFAULT || pid == PROTO_ID_NULL) {
        return None;
    }
    (object_proto_id(obj) == pid).then_some(pid)
}

/// A MIXED identity records an explicit serial link. A bare CLASS identity
/// does not pin its registry-resolved prototype, so priming resolves the live
/// declared-prototype pointer and the hit checks the registry's generation.
unsafe fn class_link(recv: *const ObjectHeader) -> Option<*const ObjectHeader> {
    let pid = shape_proto_id(object_shape_stamp(recv))?;
    if object_proto_id(recv) != pid {
        return None;
    }
    let holder = if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
        next_prototype(recv)
    } else if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        crate::object::class_decl_prototype_object((*recv).class_id)
    } else {
        return None;
    };
    (!holder.is_null() && holder != recv).then_some(holder)
}

/// The accessor's prime-time holder is still the direct prototype. Explicit
/// MIXED links are checked by pointer. Every writer that can replace a bare
/// CLASS registry link bumps the lookup-surface generation; GC rewrites both
/// this site's rooted holder and the registry root without changing it.
#[inline]
unsafe fn accessor_link_still_current(recv: *const ObjectHeader, stamp: u32, c: &PicCache) -> bool {
    let Some(pid) = shape_proto_id(stamp) else {
        return false;
    };
    if object_proto_id(recv) != pid || c[HOLDER_OBJ] as usize == recv as usize {
        return false;
    }
    if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        c[HOLDER_HOP_SHAPES] as u64 == crate::object::class_lookup_surface_generation()
    } else if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) {
        next_prototype(recv) as usize == c[HOLDER_OBJ] as usize
    } else {
        false
    }
}

struct ClassAccessor {
    holder: usize,
    shape: u32,
    slot: u32,
    raw_get: usize,
}

/// Only the direct prototype's compiled class accessor is admitted. Other
/// accessor forms keep the generic path and its receiver-override semantics.
unsafe fn class_accessor_walk(recv: *const ObjectHeader, name: &[u8]) -> Option<ClassAccessor> {
    if !holder_name_admitted(name)
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return None;
    }
    let holder = class_link(recv)? as usize;
    if !crate::value::addr_class::is_above_handle_band(holder)
        || !super::address_is_prime_stable(holder)
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(holder)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || crate::object::dictionary::is_dictionary(holder as *const ObjectHeader)
    {
        return None;
    }
    let shape = object_shape_descriptor(holder as *const ObjectHeader)?;
    if !shape.object_kind.is_ordinary_layout() {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return None;
    }
    let slot =
        crate::object::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, name)?;
    if slot >= shape.live_inline_slot_count
        || crate::object::key_attrs::keys_entry(keys, slot)
            & crate::object::key_attrs::ENTRY_ACCESSOR
            == 0
    {
        return None;
    }
    let acc = crate::object::accessor_pair::pair_of_value(slot_bits(holder, slot))?;
    if acc.raw_get == 0 && acc.raw_set == 0 {
        return None;
    }
    Some(ClassAccessor {
        holder,
        shape: object_shape_stamp(holder as *const ObjectHeader),
        slot,
        raw_get: acc.raw_get,
    })
}

unsafe fn invoke_class_getter(recv: *const ObjectHeader, raw_get: usize) -> crate::value::JSValue {
    if raw_get == 0 {
        return crate::value::JSValue::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(recv as *mut ObjectHeader);
    let f = crate::closure::body_call::js_method_body_fn!(raw_get as *const u8;);
    let bits = receiver.with_mut_ptr::<ObjectHeader, _>(|ptr| {
        let this = crate::value::js_nanbox_pointer(ptr as i64);
        f(f64::from_bits(this.to_bits())).to_bits()
    });
    crate::value::JSValue::from_bits(bits)
}

/// Collecting-path class data/absence memo; the leaf front never consults it.
pub(crate) unsafe fn try_cached_class_read(
    recv: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    class_read::try_hit(recv, cache_slot)
}

/// Collecting read-miss arm. The GC-leaf front always declines this kind.
/// Every hit confirms the receiver's shape and live link, the rooted holder's
/// shape, and the current accessor pair before invoking with the ORIGINAL receiver.
pub(crate) unsafe fn try_cached_class_accessor(
    recv: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    try_cached_class_accessor_after_worker_gate(recv, cache_slot)
}

/// The primary-agent accessor validation, also used by the unit test when
/// another test has already set the sticky process-wide worker gate.
#[inline(always)]
unsafe fn try_cached_class_accessor_after_worker_gate(
    recv: *const ObjectHeader,
    cache_slot: *mut PicCacheSlot,
) -> Option<crate::value::JSValue> {
    if cache_slot.is_null() {
        return None;
    }
    let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if cache.is_null() {
        return None;
    }
    let c = &*cache;
    if c[HOLDER_KIND] as u64 & HOLDER_ACCESSOR == 0
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
        || crate::object::field_get_set::accessor_receiver_override_armed()
        || crate::object::prototype_chain::resolution_stack_savepoint() != 0
    {
        return None;
    }
    let stamp = object_shape_stamp(recv);
    if stamp == 0
        || c[HOLDER_RECV] != (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64
        || !accessor_link_still_current(recv, stamp, c)
    {
        return None;
    }
    let holder = c[HOLDER_OBJ] as usize;
    if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
        return None;
    }
    // The holder's unchanged ShapeId carries the prime-time proof that this
    // inline slot exists and is an accessor. Key/attribute changes transition
    // the ShapeId; a raw-only pair replacement may not, so reread the pair
    // itself on every hit before invoking.
    let slot = c[HOLDER_KIND] as u32;
    let raw_get =
        crate::object::accessor_pair::raw_instance_getter_of_value(slot_bits(holder, slot))?;
    HITS_ACCESSOR.fetch_add(1, Ordering::Relaxed);
    super::stats_report_enabled();
    Some(invoke_class_getter(recv, raw_get))
}

unsafe fn walk(recv: *const ObjectHeader, name: &[u8], class_first: bool) -> Option<Walk> {
    let mut w = Walk {
        holder: 0,
        holder_shape: 0,
        slot: None,
        hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
        depth: 0,
    };
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    let mut current = recv;
    for depth in 1..=HOLDER_MAX_DEPTH {
        // `%Object.prototype%` is an immutable-prototype exotic object: its
        // [[Prototype]] is null for its whole life, whatever its shape's
        // identity word says, so reaching it ends the chain.
        let terminal = depth > 1 && current as usize == object_prototype;
        let pid = if terminal {
            PROTO_ID_NULL
        } else if depth == 1 && class_first {
            // A bare CLASS ShapeId does not pin the registry's live
            // C.prototype. The collecting class-read hit compares that
            // pointer on every use; this walk records it as the first hop.
            crate::object::shapes::PROTO_ID_CLASS
        } else if class_first {
            let pid = shape_proto_id(object_shape_stamp(current))?;
            if (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid) && object_proto_id(current) == pid {
                pid
            } else {
                admitted_proto_id(current)?
            }
        } else {
            admitted_proto_id(current)?
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
            next_prototype(current)
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
                if s >= shape.live_inline_slot_count {
                    return None;
                }
                w.holder = next as usize;
                w.holder_shape = object_shape_stamp(next);
                w.slot = Some(s);
                w.depth = depth;
                return Some(w);
            }
        }
        if depth == HOLDER_MAX_DEPTH {
            // A fifth object would be needed: either the holder or the null
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
    // A latched site keeps the caller's path.
    let existing = crate::object::field_get_set::pic_slot_peek::<PicCache>(cache_slot);
    if !existing.is_null() {
        let stamp = object_shape_stamp(obj);
        if stamp != 0 {
            let token = (u64::from(stamp) | PIC_ID_TOKEN_BIT) as i64;
            if let Some(bits) = entry_answer(&*existing, token) {
                return Some(crate::value::JSValue::from_bits(bits));
            }
        }
        if (*existing)[HOLDER_STATE] & STATE_LATCHED != 0 {
            return None;
        }
    }
    let name = crate::string::header_str_checked(key)?.as_bytes();
    let recv = ordinary_receiver(obj as usize)?;
    if let Some(acc) = class_accessor_walk(recv, name) {
        let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
        if !cache.is_null() {
            let w = Walk {
                holder: acc.holder,
                holder_shape: acc.shape,
                slot: Some(acc.slot),
                hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
                depth: 1,
            };
            publish(cache, recv, &w, true);
        }
        return Some(invoke_class_getter(recv, acc.raw_get));
    }
    // A declared class prototype is created lazily. The first getter read
    // can reach its vtable while the holder object still does not exist. Let
    // that ONE generic read materialize it without latching the site; the
    // next miss can validate the real accessor pair and publish. We never
    // call the getter twice or infer its first answer from post-call state.
    let pending_class_accessor = holder_name_admitted(name)
        && shape_proto_id(object_shape_stamp(recv))
            .is_some_and(|pid| (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid))
        && crate::object::class_decl_prototype_object((*recv).class_id).is_null()
        && std::str::from_utf8(name).ok().is_some_and(|name| {
            crate::object::class_chain_has_instance_accessor((*recv).class_id, name)
        });
    if !pending_class_accessor {
        if let Some(value) = class_read::prime(recv, key, cache_slot, name) {
            return Some(value);
        }
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
    if !pending_class_accessor
        && (!holder_name_admitted(name)
            || key_may_be_accessor(recv, name)
            || (walk(recv, name, false).is_none() && !realm_pending))
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
    if pending_class_accessor {
        if crate::object::class_decl_prototype_object((*obj).class_id).is_null() {
            let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
            refuse_and_latch(cache);
        }
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
            Some(s) => bits == slot_bits(w.holder, s) && bits != crate::value::TAG_HOLE,
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
    let old_kind = c[HOLDER_KIND] as u64;
    if !accessor
        && w.depth == 1
        && w.slot.is_none()
        && c[HOLDER_RECV] != 0
        && c[HOLDER_RECV] != token
        && (old_kind == HOLDER_ABSENT_DEPTH1 as u64 || old_kind & HOLDER_MULTI_ABSENT != 0)
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
        c[HOLDER_KIND] = (HOLDER_ABSENT_BIT
            | HOLDER_MULTI_ABSENT
            | ((next + 1) % MULTI_ABSENT_EXTRA_IDS) as u64) as i64;
        c[HOLDER_RECV] = token;
        PRIMES_ABSENT.fetch_add(1, Ordering::Relaxed);
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
    c[HOLDER_RECV] = 0;
    c[HOLDER_OBJ] = w.holder as i64;
    c[HOLDER_SHAPE] = (u64::from(w.holder_shape) | u64::from(w.hops[2].1) << 32) as i64;
    c[HOLDER_KIND] = if accessor {
        (HOLDER_ACCESSOR | u64::from(w.slot.expect("class accessor has slot"))) as i64
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
    c[HOLDER_HOP_SHAPES] = if accessor {
        crate::object::class_lookup_surface_generation() as i64
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
                for i in 0..HOLDER_MAX_DEPTH - 1 {
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

    /// Two holders with exactly one ShapeId but different compiled getters.
    /// Replacing a declared class's registry pointer leaves the receiver's
    /// bare CLASS ShapeId unchanged; the collecting hit must compare the live
    /// link, rather than trust receiver and holder shapes alone.
    #[test]
    fn class_accessor_rechecks_same_shape_holder_link() {
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
            let first = unsafe { class_accessor_walk(receiver, b"path") }.expect("first accessor");
            let cache: &'static mut PicCache =
                Box::leak(Box::new([0; crate::codegen_abi::PIC_CACHE_WORDS]));
            let mut slot: PicCacheSlot = cache;
            let w = Walk {
                holder: first.holder,
                holder_shape: first.shape,
                slot: Some(first.slot),
                hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
                depth: 1,
            };
            unsafe { publish(cache, receiver, &w, true) };
            let first_generation = cache[HOLDER_HOP_SHAPES];
            assert_eq!(
                first_generation as u64,
                crate::object::class_lookup_surface_generation()
            );
            assert_eq!(
                unsafe { try_cached_class_accessor_after_worker_gate(receiver, &mut slot) }
                    .map(|v| v.as_number()),
                Some(2.0)
            );

            let old_relinks = read_accessor_same_shape_relinks();
            p2.with_const_ptr::<ObjectHeader, _>(|ptr| {
                crate::object::test_seed_class_decl_prototype_object_root(CID, ptr as usize);
            });
            assert_ne!(
                cache[HOLDER_HOP_SHAPES] as u64,
                crate::object::class_lookup_surface_generation()
            );
            assert_eq!(unsafe { object_shape_stamp(receiver) }, recv_shape);
            assert!(
                unsafe { try_cached_class_accessor_after_worker_gate(receiver, &mut slot) }
                    .is_none(),
                "stale getter was served after registry replacement"
            );
            let second =
                unsafe { class_accessor_walk(receiver, b"path") }.expect("second accessor");
            let w = Walk {
                holder: second.holder,
                holder_shape: second.shape,
                slot: Some(second.slot),
                hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
                depth: 1,
            };
            unsafe { publish(cache, receiver, &w, true) };
            assert!(read_accessor_same_shape_relinks() > old_relinks);
            assert_eq!(
                unsafe { try_cached_class_accessor_after_worker_gate(receiver, &mut slot) }
                    .map(|v| v.as_number()),
                Some(8.0)
            );
        });
    }

    #[test]
    fn ten_receiver_shapes_share_one_confirmed_absent_terminal() {
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
            hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
            depth: 1,
        };
        for i in 0..11 {
            recv.parent_class_id = base + i;
            unsafe { publish(&mut cache, &recv, &absent, false) };
        }
        assert_ne!(cache[HOLDER_KIND] as u64 & HOLDER_MULTI_ABSENT, 0);
        assert_eq!(
            unsafe {
                entry_answer_after_worker_gate(&cache, (PIC_ID_TOKEN_BIT | u64::from(base)) as i64)
            },
            None,
            "the oldest of eleven shapes must leave a ten-shape site"
        );
        for i in 1..11 {
            let token = (PIC_ID_TOKEN_BIT | u64::from(base + i)) as i64;
            assert_eq!(
                unsafe { entry_answer_after_worker_gate(&cache, token) },
                Some(crate::value::TAG_UNDEFINED)
            );
        }
        // A new shape after an own-key shadow has no entry, while a terminal
        // mutation invalidates every receiver shape in the shared entry.
        assert_eq!(
            unsafe {
                entry_answer_after_worker_gate(
                    &cache,
                    (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64,
                )
            },
            None
        );
        let mut moved = Box::new(ObjectHeader {
            class_id: 0,
            parent_class_id: base + 100,
            meta: std::ptr::null_mut(),
        });
        cache[HOLDER_OBJ] = (&mut *moved as *mut ObjectHeader) as i64;
        assert_eq!(
            unsafe {
                entry_answer_after_worker_gate(
                    &cache,
                    (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64,
                )
            },
            Some(crate::value::TAG_UNDEFINED)
        );
        moved.parent_class_id = base + 102;
        assert_eq!(
            unsafe {
                entry_answer_after_worker_gate(
                    &cache,
                    (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64,
                )
            },
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
            unsafe {
                entry_answer_after_worker_gate(
                    &cache,
                    (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64,
                )
            },
            None
        );
        assert_eq!(
            unsafe {
                entry_answer_after_worker_gate(
                    &cache,
                    (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64,
                )
            },
            Some(crate::value::TAG_UNDEFINED)
        );
    }

    /// A class instance has a valid, stamped ShapeId, but its prototype is
    /// resolved through the class vtable. The holder walk must refuse it even
    /// when the shape and the object's current prototype id agree.
    #[test]
    fn class_prototype_identity_is_refused_by_read_holder() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CLASS_ID: u32 = 0x0C3C_79A2;
        let packed = b"holder_class_key";
        let keys = crate::object::js_build_class_keys_array(
            CLASS_ID,
            1,
            packed.as_ptr(),
            packed.len() as u32,
            0,
        );
        let shape_id = crate::object::shapes::js_object_shape_id_for_class_keys(
            keys as usize as u64,
            1,
            CLASS_ID,
            0,
        );
        let obj = crate::object::js_object_alloc_class_inline_keys_stamped(
            CLASS_ID, 0, 1, keys, shape_id, 0,
        );
        let claimed = shape_proto_id(shape_id).expect("class shape must be stamped");
        assert_eq!(claimed, crate::object::shapes::class_proto_id(CLASS_ID));
        assert_eq!(unsafe { object_proto_id(obj) }, claimed);
        assert!(claimed >= crate::object::shapes::PROTO_ID_CLASS);
        assert_eq!(unsafe { admitted_proto_id(obj) }, None);
    }
}
