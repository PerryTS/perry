//! The read site's HOLDER entry: `o.k` where `k` is not an own key of the
//! receiver, answered by facts of two shapes.
//!
//! * The receiver's ShapeId `S` vouches that `k` is not own, that the receiver
//!   is an ordinary object, and its [[Prototype]] identity. Only a serial
//!   identity or `PROTO_ID_DEFAULT` (the realm's `Object.prototype`) pins ONE
//!   object, so only those admit.
//! * The holder's ShapeId `SH` vouches that `k` is an own inline data slot of
//!   the holder `H` — or, for an ABSENT entry, that the terminal object lacks
//!   `k` and has a null [[Prototype]].
//! * For a holder deeper than the direct prototype, each intermediate hop's
//!   ShapeId vouches that the hop lacks `k` and still links to the next hop.
//!
//! Every fact is compared on use, so there is no invalidation and no global
//! word: a key add, delete, descriptor change or `setPrototypeOf` on any object
//! the entry names moves that object's ShapeId, and a value store to the
//! holder's slot is seen because the hit LOADS the slot. A delete is a shape
//! transition (#10826), so a holder whose ShapeId matches still has the slot:
//! the hit needs no `TAG_HOLE` test, as the emitted MRU hit needs none.
//!
//! The entry lives in the read site's own cache (`PicCache` words
//! [`HOLDER_RECV`]..=[`HOLDER_REGISTERED`]). The holder and the hops are
//! STRONG roots, rewritten when they move ([`scan_read_holder_roots_mut`]).
//!
//! # Emitted form
//!
//! Codegen (`generic_dispatch.rs`) checks the entry on the edges where the
//! MRU word and the polymorphic ways have missed. A depth-1 entry is compared
//! and loaded inline; a deeper one calls [`js_read_site_holder_hit`], a
//! GC leaf that compares the hop words and answers `TAG_HOLE` to decline.
//!
//! # Priming
//!
//! Only from the read miss handler, which already knows the key is not own,
//! and only after the generic getter has produced the answer: the entry is
//! recorded only when what the shapes say equals what the getter returned
//! (names the runtime synthesizes, lazily materialized intrinsics and
//! `constructor` refuse there). Primary agent only; a worker agent's start
//! empties every entry.
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
    object_proto_id, object_shape_descriptor, object_shape_stamp, shape_proto_id, ShapeObjectKind,
    PIC_ID_TOKEN_BIT, PROTO_ID_DEFAULT, PROTO_ID_NULL,
};
use crate::object::{ObjectHeader, PicCache, PicCacheSlot};
use std::sync::atomic::{AtomicU64, Ordering};

/// The receiver's ShapeId as a PIC token (`ShapeId | PIC_ID_TOKEN_BIT`), or 0
/// for an empty entry. A zeroed cache is therefore an empty one: no token is 0.
pub const HOLDER_RECV: usize = crate::codegen_abi::PIC_HOLDER_RECV_WORD;
/// The holder's (or, for an absent entry, the terminal object's) address.
pub const HOLDER_OBJ: usize = crate::codegen_abi::PIC_HOLDER_OBJ_WORD;
/// Low 32 bits: the holder's ShapeId. High 32 bits: the third hop's ShapeId.
pub const HOLDER_SHAPE: usize = crate::codegen_abi::PIC_HOLDER_SHAPE_WORD;
/// The answer's kind, laid out for the emitted test:
///
/// | value | meaning |
/// |---|---|
/// | `0 ..= u32::MAX` | depth 1, the value is the holder's inline slot |
/// | [`HOLDER_ABSENT_DEPTH1`] | depth 1, absent: the answer is `undefined` |
/// | negative | [`HOLDER_STUB`] set: call [`js_read_site_holder_hit`] |
pub const HOLDER_KIND: usize = crate::codegen_abi::PIC_HOLDER_KIND_WORD;
/// First of three intermediate hop addresses (depth 2..=4).
pub const HOLDER_HOPS: usize = HOLDER_KIND + 1;
/// Low 32 bits: the first hop's ShapeId. High 32 bits: the second hop's.
pub const HOLDER_HOP_SHAPES: usize = HOLDER_HOPS + 3;
/// The site's holder state: [`STATE_REGISTERED`], [`STATE_LATCHED`] and the
/// count of re-primes for a different receiver shape.
pub const HOLDER_STATE: usize = HOLDER_HOP_SHAPES + 1;
/// The cache is on the root list.
const STATE_REGISTERED: i64 = 1;
/// The site refused, or is polymorphic in its non-own receivers: no walk and
/// no prime from here on.
const STATE_LATCHED: i64 = 2;
const STATE_REPRIME_SHIFT: u32 = 8;
/// Re-primes for a different receiver shape a site takes before it latches.
const MAX_REPRIMES: i64 = 4;

pub const HOLDER_ABSENT_DEPTH1: i64 = crate::codegen_abi::PIC_HOLDER_ABSENT_DEPTH1;
pub const HOLDER_STUB: u64 = 1 << 63;
const HOLDER_ABSENT_BIT: u64 = 1 << 62;
const HOLDER_DEPTH_SHIFT: u32 = 32;
const HOLDER_MAX_DEPTH: usize = 4;

/// Every cache that holds (or held) a holder entry, for the root scan and for
/// emptying the entries when the first worker agent starts. The entries are
/// in the per-site caches; this is only where the scan finds them.
static HOLDER_SITES: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

per_test_global! {
    static PRIMES_HOLDER: AtomicU64 = AtomicU64::new(0);
    static PRIMES_ABSENT: AtomicU64 = AtomicU64::new(0);
    static REFUSED_HOLDER: AtomicU64 = AtomicU64::new(0);
}

/// `(data primes, absent primes, refusals)`.
pub fn read_holder_stats() -> (u64, u64, u64) {
    (
        PRIMES_HOLDER.load(Ordering::Relaxed),
        PRIMES_ABSENT.load(Ordering::Relaxed),
        REFUSED_HOLDER.load(Ordering::Relaxed),
    )
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
unsafe fn entry_answer(c: &PicCache, token: i64) -> Option<u64> {
    if c[HOLDER_RECV] != token || token == 0 {
        return None;
    }
    let kind = c[HOLDER_KIND];
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

/// The site's holder entry asked for `handle`, without priming: what the
/// emitted tower's holder check answers, for a runtime caller that asks the
/// site's words itself (`typed_feedback::guards`' class-field miss arm).
///
/// # Safety
/// `handle` is a pointer above the handle band; `cache_slot` null or the
/// site's live read cache slot.
#[inline]
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
unsafe fn admitted_proto_id(obj: *const ObjectHeader) -> Option<u64> {
    let pid = shape_proto_id(object_shape_stamp(obj))?;
    let serial = pid != PROTO_ID_DEFAULT && pid < crate::object::shapes::PROTO_ID_CLASS;
    if !(serial || pid == PROTO_ID_DEFAULT || pid == PROTO_ID_NULL) {
        return None;
    }
    (object_proto_id(obj) == pid).then_some(pid)
}

unsafe fn walk(recv: *const ObjectHeader, name: &[u8]) -> Option<Walk> {
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
        let next = if pid == PROTO_ID_DEFAULT {
            object_prototype as *const ObjectHeader
        } else {
            next_prototype(current)
        };
        if next.is_null() || next == current || next == recv || !hop_admitted(next as usize, name) {
            return None;
        }
        let shape = object_shape_descriptor(next)?;
        if shape.object_kind != ShapeObjectKind::Ordinary || object_shape_stamp(next) == 0 {
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
        || WORKER_AGENTS_EXIST.load(Ordering::SeqCst)
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
    // Cheap pre-walk: a receiver the entry could never describe keeps the
    // caller's path and pays nothing for the getter below. A site with no
    // cache yet stays without one, so the emitted never-primed edge keeps its
    // inherited-read hook.
    if !holder_name_admitted(name) || key_may_be_accessor(recv, name) || walk(recv, name).is_none()
    {
        refuse_and_latch(existing);
        return None;
    }

    // The answer, from the generic getter. It can run user code and collect,
    // so the receiver is rooted across it and everything is re-read after.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj as *mut ObjectHeader);
    let (value, obj) = handle.across_mut::<ObjectHeader, _>(|| {
        crate::object::field_get_set::get_field_by_name_past_inherited_cache(obj, key)
    });
    // From here a refusal has already run the getter, so the site latches:
    // the next miss must not walk and run it again only to refuse again.
    let cache = crate::object::field_get_set::pic_slot_resolve::<PicCache>(cache_slot);
    let name = crate::string::header_str_checked(key)?.as_bytes();
    let Some(recv) = ordinary_receiver(obj as usize) else {
        refuse_and_latch(cache);
        return Some(value);
    };
    let Some(w) = walk(recv, name) else {
        refuse_and_latch(cache);
        return Some(value);
    };
    // Confirm: what the shapes say must be what the getter returned.
    let bits = value.bits();
    let confirmed = match w.slot {
        None => bits == crate::value::TAG_UNDEFINED,
        Some(s) => bits == slot_bits(w.holder, s) && bits != crate::value::TAG_HOLE,
    };
    if !confirmed {
        refuse_and_latch(cache);
        return Some(value);
    }
    if cache.is_null() {
        return Some(value);
    }
    publish(cache, recv, &w);
    Some(value)
}

unsafe fn publish(cache: *mut PicCache, recv: *const ObjectHeader, w: &Walk) {
    let c = &mut *cache;
    let token = (u64::from(object_shape_stamp(recv)) | PIC_ID_TOKEN_BIT) as i64;
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
    c[HOLDER_RECV] = 0;
    c[HOLDER_OBJ] = w.holder as i64;
    c[HOLDER_SHAPE] = (u64::from(w.holder_shape) | u64::from(w.hops[2].1) << 32) as i64;
    c[HOLDER_KIND] = match (w.depth, w.slot) {
        (1, Some(s)) => i64::from(s),
        (1, None) => HOLDER_ABSENT_DEPTH1,
        (d, s) => {
            (HOLDER_STUB
                | if s.is_none() { HOLDER_ABSENT_BIT } else { 0 }
                | (d as u64) << HOLDER_DEPTH_SHIFT
                | u64::from(s.unwrap_or(0))) as i64
        }
    };
    for i in 0..HOLDER_MAX_DEPTH - 1 {
        c[HOLDER_HOPS + i] = w.hops[i].0 as i64;
    }
    c[HOLDER_HOP_SHAPES] = (u64::from(w.hops[0].1) | u64::from(w.hops[1].1) << 32) as i64;
    if c[HOLDER_STATE] & STATE_REGISTERED == 0 {
        c[HOLDER_STATE] |= STATE_REGISTERED;
        if let Ok(mut sites) = HOLDER_SITES.lock() {
            sites.push(cache as usize);
        }
    }
    // Last: the entry is live only once every other word is written.
    c[HOLDER_RECV] = token;
    if w.slot.is_some() {
        PRIMES_HOLDER.fetch_add(1, Ordering::Relaxed);
    } else {
        PRIMES_ABSENT.fetch_add(1, Ordering::Relaxed);
    }
    super::stats_report_enabled();
}

/// The emitted form's call for an entry it does not answer inline (depth 2..4,
/// or absent past depth 1). A GC leaf: it reads site words and object words
/// only. `TAG_HOLE` declines, and the caller continues to the miss call.
///
/// # Safety
/// `recv` is the receiver the emitted tower validated as a plain object;
/// `cache` the site's resolved `PicCache`.
#[no_mangle]
pub unsafe extern "C" fn js_read_site_holder_hit(recv: *const u8, cache: *const PicCache) -> f64 {
    if cache.is_null() || recv.is_null() {
        return f64::from_bits(crate::value::TAG_HOLE);
    }
    let token = (u64::from(std::ptr::read((recv as *const u32).add(1))) | PIC_ID_TOKEN_BIT) as i64;
    f64::from_bits(entry_answer(&*cache, token).unwrap_or(crate::value::TAG_HOLE))
}

/// Root scan: every live entry's holder and hops are marked and rewritten.
pub(crate) fn scan_read_holder_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let Ok(sites) = HOLDER_SITES.lock() else {
        return;
    };
    for &site in sites.iter() {
        // SAFETY: registered caches are PIC-arena allocations
        // (`pic_arena_alloc`), which are never freed.
        let c = unsafe { &mut *(site as *mut PicCache) };
        if c[HOLDER_RECV] == 0 {
            continue;
        }
        visitor.visit_i64_slot(&mut c[HOLDER_OBJ]);
        for i in 0..HOLDER_MAX_DEPTH - 1 {
            visitor.visit_i64_slot(&mut c[HOLDER_HOPS + i]);
        }
    }
}

/// The first worker agent's start: a holder entry names a primary-heap object,
/// so every entry is emptied, and none is primed again (`WORKER_AGENTS_EXIST`).
pub(crate) fn empty_read_holder_entries() {
    let Ok(sites) = HOLDER_SITES.lock() else {
        return;
    };
    for &site in sites.iter() {
        // SAFETY: as in `scan_read_holder_roots_mut`; one aligned word store.
        unsafe {
            std::ptr::write_volatile(&mut (*(site as *mut PicCache))[HOLDER_RECV], 0);
        }
    }
}
