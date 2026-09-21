//! Inherited-read cache: `(receiver shape, key) -> (holder, inline slot)`.
//!
//! # The hole this fills
//!
//! Every property inline cache in Perry serves OWN data properties only. A
//! read whose key lives on the prototype chain therefore misses every cache
//! and re-runs the whole generic getter, which walks the chain from scratch on
//! every read. Callgrind on `const P={a:1}; const O=Object.create(P); O.a` (a
//! one-hop chain, perfectly monomorphic, v0.5.1619, 200 k reads):
//!
//! | cost per read | instr |
//! |---|---|
//! | `native_get::try_data_get_bytes` self (the probe ladder) | 344 |
//! | `class_registry::prototype_objects::class_prototype_object` | 192 (118 of it SipHash) |
//! | `keys_find_slot_by_bytes_resolved` x2 (receiver keys, then holder keys) | 180 |
//! | `ic_miss::get_field_ic_miss_impl` self (the own-key search that must fail first) | 153 |
//! | `has_property::closure_dynamic_prop_by_key` | 90 |
//! | `shapes::shape_descriptor_by_id` | 70 |
//! | `is_anon_shape_class_id` / `class_decl_prototype_object` / `from_utf8` / `is_arguments_object` | 161 |
//! | total | ~1300 |
//!
//! node and bun both serve the same read for the same price as an own read
//! (measured: 16.2 vs 16.0, and 14.2 vs 13.3). Nothing in that 1300 is a
//! prototype MUTEX or an address-keyed prototype probe — `OBJECT_PROTOTYPES`
//! never appears in the profile, because #6759 phase B already moved shaped
//! objects onto `ObjectMeta.prototype`. The cost is the walk itself being
//! redone, plus a SipHash class-registry probe per read to find the prototype
//! of an `Object.create` receiver.
//!
//! # What an entry claims, and what makes the claim true
//!
//! An entry says: *a receiver whose (class id, ShapeId, recorded prototype
//! bits) are these, reading this interned key, finds it as a plain inline data
//! slot `slot` on `hops[hop_count-1]`, having passed through `hops[0..]` in
//! order.*
//!
//! The claim is re-proved on every hit by all four checks below. The code
//! runs them cheapest-and-most-selective first, not in the order they are
//! listed:
//!
//! 1. `proto_validity::proto_validity()` is unchanged. ONE global word, ONE
//!    load, ONE compare, covering a chain of ANY depth. It stands for two
//!    things at once:
//!    * no object anybody inherits from has changed STRUCTURALLY. An object
//!      is marked (`OBJ_FLAG_IS_PROTOTYPE`) when this cache records it as a
//!      hop, and every shape-word change on a marked object bumps the counter
//!      from the runtime's single structural-mutation publication funnel. A
//!      key ADDED to a prototype bumps no epoch — a plain store is not a
//!      descriptor install — and this is what sees it.
//!    * nothing the semantic property epoch stands for has happened:
//!      descriptor installs and clears, `delete`, per-instance prototype
//!      recording (`Object.setPrototypeOf`, `__proto__`),
//!      class-prototype-object registration, parent-static linking.
//!      `prop_plan_epoch_bump` bumps this word too. It is deliberately NOT
//!      bumped by GC, which is what keeps this cache off the #7910 cliff (the
//!      full `PROP_PLAN_EPOCH` is bumped at loop-poll cadence by the
//!      incremental collector, so keying on it degrades any cache into an
//!      unconditional recompute).
//!
//!    This REPLACED one ShapeId compare per hop. The per-hop walk was correct
//!    and it had two costs: it was proportional to chain depth, up to four
//!    dependent loads through prototype objects that are usually cold; and it
//!    was a LOOP, so the hit could only ever live behind a call. An emitted
//!    read site cannot branch on a variable number of compares.
//! 2. The receiver's `(class_id, ShapeId)` pair — ONE aligned 8-byte load at
//!    offset 0 — equals what was recorded. This is what makes a shadowing own
//!    key safe: adding `o.a` to the receiver is a key-add transition, which
//!    mints a different ShapeId, so the entry simply stops matching. Priming
//!    requires the key to be absent from the receiver's key list ENTIRELY (not
//!    present as a `TAG_HOLE` tombstone), so no stable-tombstone re-add can
//!    reinstate an own key under an unchanged ShapeId.
//! 3. The receiver's recorded prototype bits (`ObjectMeta.prototype`, 0 when
//!    there is no meta record) are unchanged. This is NOT redundant with the
//!    validity word: `object_link_class_default_prototype` links a FRESH
//!    instance to its class's prototype object without bumping any epoch and
//!    without transitioning a shape (by design — the loud variant flushed the
//!    plan cache on every construction). A later `C.prototype = other`
//!    followed by `new C()` therefore produces a receiver with the SAME class
//!    id and the SAME ShapeId as the cached one and a different chain, and
//!    this compare is what refuses it.
//! 4. The key pointer is identical. Keys are interned, so pointer identity is
//!    key identity — see the GC contract below for why the pointer cannot be
//!    reused by a different string while an entry names it.
//!
//! Conditions that are proved ONCE, at prime time, and held afterwards by the
//! validity word rather than re-checked: object kind is `Ordinary`; no
//! accessor and no customized descriptor for this key anywhere on the chain
//! (both transition the shape of the object they are installed on, and every
//! hop is marked, so either would have bumped the counter); the slot is inline
//! rather than spilled.
//!
//! Conditions re-checked on every hit because they are properties of the
//! ADDRESS rather than of the shape, and all three now live in ONE word this
//! path already loads (`ObjectMeta::flags`, plus `elements` beside it): the
//! receiver is not `process.env`, not an `arguments` object, and carries no
//! `elements` store. The first two were two address-keyed registry probes
//! costing 14.0 and 5.0 instructions on every cached read — and, decisively
//! for the emitted sequence, a compiled read site could not have called
//! either one.
//!
//! # GC contract
//!
//! The collector moves objects, so a recorded holder address is a liability.
//! Three things together make it safe, and each is necessary:
//!
//! * **`scan_inherited_read_cache_roots_mut` MARKS every key, hop and the
//!   holder**, so the
//!   collector keeps them alive and rewrites this table's copy of their
//!   addresses. A hit LOADS `holder + slot`, which is what separates this
//!   cache from the transition cache #6759 phase 3 made weak: that one only
//!   ever COMPARES addresses, so a dangling slot costs it a miss, while here
//!   it would be a read of recycled memory returning a wrong value. The
//!   retention is bounded by the table (512 keys, 512 x 4 prototypes).
//! * **`prune_dead_inherited_cache_entries` drops entries whose key or any hop
//!   the collector reports dead.** Marking above means it should never have
//!   one to drop; it is registered in `DEAD_KEY_PRUNES` anyway because that
//!   registry is the runtime's checked list of tables re-keyed by a visitor
//!   (`gc::dead_owner`'s #8174 note), and it is the only thing standing
//!   between a recycled address and a false hit should a collection flavour
//!   ever sweep without running the scanner. `young_prune: None`, so the full
//!   512-entry walk runs on every flavour including minors - cheap at this
//!   size.
//! * **Priming does NOT refuse a nursery hop.** It did, and that made the
//!   cache useless for the programs that need it most: a read-only loop
//!   allocates nothing, nothing is ever promoted, and every prototype stays in
//!   the nursery for the life of the process. Measured cost of that refusal:
//!   +264 instructions per inherited read, for a chain walk performed and then
//!   discarded.
//!
//! # Refusals are recorded too
//!
//! Most reads that reach this cache are ones it cannot serve: an accessor on
//! the prototype, a key that is on no prototype at all, a receiver whose kind
//! it refuses. If a refusal is not recorded, each of those pays for a full
//! chain walk on EVERY read and the cache is a net loss — measured at +424
//! instructions per read for an accessor on the prototype, against a build
//! with no cache at all.
//!
//! So a declining walk writes a NEGATIVE entry (`slot == NEGATIVE_SLOT`) and
//! [`inherited_read_cache_lookup`] answers `Declined`, which tells
//! `get_field_ic_miss_impl` not to walk. A negative entry can never return a
//! wrong value — the worst it can do is keep a read on the path it is already
//! on — so its invalidation may be weaker than a hit's. It reuses the hit's
//! identity, epoch and per-hop stamp compares unchanged, which is what lets
//! `proto.a = 1` after a failed lookup re-open the pair.
//!
//! One class of refusal is NOT recorded: one caused by a VALUE — an
//! `undefined`, `null` or hole in the holder's slot. A plain store can replace
//! such a value with a real one while transitioning no shape and bumping no
//! epoch, so a negative entry for it would stand for the life of the process.
//! Every other refusal is a function of a shape or a descriptor.
//!
//! # Why every hop is still recorded when only the holder is read
//!
//! The hit loads `holder + slot` and never touches an interior hop, so the
//! `hops` array exists for the collector, not for the read. Dropping the
//! interior hops would make this entry able to hit for a receiver whose
//! recorded prototype bits name an address the collector recycled: the
//! intermediate prototype is kept alive by the receiver in every real
//! program, but this table does not root receivers, so nothing else is
//! keeping it from being freed and its address reused. Rooting the whole
//! chain costs GC time and nothing at read time.
//!
//! # Hook points for lane 4
//!
//! Two, both one call wide:
//!   * [`inherited_read_cache_hit`] — the guard-and-load. Called at the top of
//!     `js_object_get_field_by_name` and of `get_field_ic_miss_impl`.
//!   * [`inherited_read_cache_prime`] — the chain walk. Called from
//!     `get_field_ic_miss_impl` only, at the point where the own-key search has
//!     already failed, so it never duplicates work on an own read.
//!
//! An emitted-code hit would call `js_inherited_read_cache_hit_f64`, the
//! `extern "C"` wrapper at the bottom of this file, with the receiver's masked
//! pointer and the interned key.

use super::{shapes, ObjectHeader};
use crate::value::JSValue;
use std::sync::atomic::{AtomicU64, Ordering};

/// Direct-mapped, per thread. 512 entries x 88 bytes is 45 KB; the table is
/// boxed for the same reason `prop_plan`'s is (an oversized inline TLS block
/// overflows the ILP32 TLS layout on arm64_32).
const CACHE_SIZE: usize = 512;
const CACHE_MASK: usize = CACHE_SIZE - 1;

/// Longest chain an entry can describe. `Object.create(Object.create(...))`
/// towers beyond this decline and keep today's walk. Four covers an instance
/// reaching `Object.prototype` through two user levels.
const MAX_HOPS: usize = 4;

#[derive(Clone, Copy)]
struct Entry {
    /// Interned key pointer. 0 marks the slot empty.
    key_ptr: usize,
    /// `proto_validity::proto_validity()` at prime time.
    validity: u64,
    /// The receiver's `ObjectMeta.prototype` at prime time, 0 for no record.
    recv_proto_bits: u64,
    recv_class_id: u32,
    recv_shape: u32,
    /// The object the key was found on: `hops[hop_count - 1]`, kept in its own
    /// field so the hit loads it at a fixed offset instead of indexing.
    holder: usize,
    /// Chain from the receiver's prototype (`hops[0]`) to the holder. Read by
    /// the GC hooks only; the hit never walks it.
    hops: [usize; MAX_HOPS],
    hop_count: u8,
    /// Inline field index on the holder. Spilled fields never prime.
    slot: u32,
}

/// `slot` for a NEGATIVE entry: one that records that a walk from this
/// (receiver shape, key) pair declined, so the walk is not re-run on every
/// read. A real slot is an inline field index, so this value cannot collide
/// with one.
///
/// Without it, every read the cache REFUSES pays for the refusal: an accessor
/// on the prototype measured 2912 instructions per loop with no cache and
/// 3336 with one, because the chain walk ran and was thrown away every time.
/// A negative entry is never wrong — it only ever says "do what you did
/// before" — so its invalidation may be weaker than a hit's, and it reuses
/// the hit's identity, epoch and per-hop compares unchanged.
/// **REQUIREMENT for any sentinel added beside this one.** Keep it at the TOP
/// of the `u32` range, adjacent to this value and descending.
///
/// A real slot is an inline field index, bounded by the shape's
/// `live_inline_slot_count`. The emitted per-site publication being built for
/// inherited reads may publish an entry ONLY when it names a real holder and
/// slot, and with every sentinel above every valid index that predicate is a
/// single unsigned compare (`slot < live_inline_slot_count`) which excludes
/// all of them by construction. A sentinel placed anywhere else turns that one
/// compare into an enumeration that has to be extended every time somebody
/// adds a verdict — and the failure mode of forgetting is publishing a
/// sentinel to a site as if it were a field offset.
const NEGATIVE_SLOT: u32 = u32::MAX;

/// What a table lookup found.
pub(crate) enum Lookup {
    /// An entry proved its claim; this is the value.
    Hit(JSValue),
    /// A walk from this pair declined last time, under conditions that still
    /// hold. The caller must not walk again.
    Declined,
    /// Nothing recorded.
    Unknown,
}

const EMPTY_ENTRY: Entry = Entry {
    key_ptr: 0,
    validity: 0,
    recv_proto_bits: 0,
    recv_class_id: 0,
    recv_shape: 0,
    holder: 0,
    hops: [0; MAX_HOPS],
    hop_count: 0,
    slot: 0,
};

crate::perry_thread_local! {
    static INHERITED_READ_CACHE: std::cell::UnsafeCell<Box<[Entry]>> =
        std::cell::UnsafeCell::new(vec![EMPTY_ENTRY; CACHE_SIZE].into_boxed_slice());
}

/// An entry is identified by (class id, ShapeId, key), so all three have to
/// reach the slot index.
///
/// #10834 hashed only (shape, key). That is exactly wrong for the receivers
/// this cache exists to serve: `js_object_create` mints a FRESH synthetic
/// class id on every call, so N objects built by `Object.create(p)` have N
/// different class ids and ONE identical shape. Under a (shape, key) index
/// they all landed in the same direct-mapped slot and evicted one another, so
/// a site reading through eight of them primed on EVERY read and hit never:
/// measured `primes=6295655 hits=0` over ten million reads, a full chain walk
/// plus an entry write per read, +75 instructions against the same binary with
/// the cache off.
#[inline(always)]
fn entry_index(class_id: u32, shape: u32, key_ptr: usize) -> usize {
    // Interned key pointers are 8- or 16-byte aligned, so their low bits are
    // zeros; fold the middle bits down before masking.
    let h = ((key_ptr >> 4) as u64 ^ ((shape as u64) << 21) ^ ((class_id as u64) << 43))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 40) as usize & CACHE_MASK
}

/// `PERRY_INHERITED_IC=0` turns the cache off in a binary that has it, so the
/// same build can be measured with and without one environment variable apart
/// (the discipline `PERRY_IC_OUTLINE_FASTPATH` established). Nothing branches
/// on it for behaviour: both settings answer identically.
#[inline]
fn cache_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        crate::gc::env_default_on_from_value(std::env::var("PERRY_INHERITED_IC").ok().as_deref())
    })
}

// --- hit counters -----------------------------------------------------------
//
// A cache that silently falls through to the walk is correct-but-slow and
// invisible in a program's OUTPUT. These make the hit itself assertable.

static HITS: AtomicU64 = AtomicU64::new(0);
static PRIMES: AtomicU64 = AtomicU64::new(0);
static DECLINES: AtomicU64 = AtomicU64::new(0);
/// Declines answered from a NEGATIVE entry, i.e. without walking. Separate
/// from `DECLINES` because "the cache refused" and "the cache refused for the
/// price of one lookup" are different facts, and only the second one is the
/// claim `NEGATIVE_SLOT` exists to make.
static NEG_SERVED: AtomicU64 = AtomicU64::new(0);

/// Counting is gated at the CALL SITE by this latch so a default-off
/// diagnostic costs one predictable branch, not an atomic add, per read.
/// Tests need the counters whatever environment they run under, and a
/// `cfg(test)` build is not a build anyone measures.
///
/// `PERRY_IC_DIAG` arms it too, and `IcDiag::render` prints the three counts
/// in the `[ic-diag]` report: the existing instrument for "what did the
/// property-read caches actually do", extended rather than duplicated. Both
/// inputs are read from the environment once, so this stays one `OnceLock`
/// load however it was armed.
#[inline]
fn stats_enabled() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| {
            std::env::var_os("PERRY_INHERITED_IC_STATS").is_some() || crate::hot_diag::ic_on()
        })
    }
}

pub(crate) fn inherited_read_cache_hits() -> u64 {
    HITS.load(Ordering::Relaxed)
}

pub(crate) fn inherited_read_cache_primes() -> u64 {
    PRIMES.load(Ordering::Relaxed)
}

pub(crate) fn inherited_read_cache_declines() -> u64 {
    DECLINES.load(Ordering::Relaxed)
}

pub(crate) fn inherited_read_cache_neg_served() -> u64 {
    NEG_SERVED.load(Ordering::Relaxed)
}

/// Counters for a caller that wants a delta over a region (tests).
#[cfg(test)]
pub(crate) fn test_reset_counters() {
    HITS.store(0, Ordering::Relaxed);
    PRIMES.store(0, Ordering::Relaxed);
    DECLINES.store(0, Ordering::Relaxed);
    NEG_SERVED.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_clear_cache() {
    INHERITED_READ_CACHE.with(|cell| unsafe {
        for entry in (*cell.get()).iter_mut() {
            *entry = EMPTY_ENTRY;
        }
    });
}

// --- shared predicates ------------------------------------------------------

/// The per-object facts a ShapeId does NOT pin, checked on the receiver at
/// both prime and hit time.
///
/// `process.env` is an OS-backed exotic object and an `arguments` object has
/// its own index semantics; both are plain `GC_TYPE_OBJECT`s whose key list —
/// and therefore whose ShapeId — an ordinary object can coincide with. An
/// `elements` store is array-subclass backing that answers reads before the
/// shape does.
#[inline]
unsafe fn receiver_address_facts_ok(meta: *const crate::object::ObjectMeta) -> bool {
    if meta.is_null() {
        return true;
    }
    (*meta).elements == 0
        && (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0
}

/// An address a prime may record: one this heap knows, so its `GcHeader` is
/// readable and the collector's hooks reach it.
///
/// This deliberately does NOT restrict priming to the old generation. It did,
/// on the argument that a minor can then never invalidate an entry — and that
/// made the cache useless for precisely the programs that need it most. A loop
/// that only READS allocates nothing, so nothing is ever promoted, so every
/// prototype stays in the nursery for the life of the process and every prime
/// declined: measured +264 instructions per inherited read (1433 -> 1697) for
/// a walk that was performed and then thrown away. Nursery hops are safe
/// because the two GC hooks below cover them, which is the same contract every
/// other address-recording cache in the runtime lives under.
#[inline]
fn address_is_prime_stable(addr: usize) -> bool {
    crate::value::addr_class::is_plausible_heap_addr(addr)
        && crate::arena::classify_heap_generation(addr) != crate::arena::HeapGeneration::Unknown
}

// --- the hit ----------------------------------------------------------------

/// Serve `obj.key` from a cached inherited entry, or decline.
///
/// # Safety
/// `obj` is a masked, non-null heap pointer the caller has already established
/// is a plausible heap address; `key` may be null.
#[inline]
pub(crate) unsafe fn inherited_read_cache_hit(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    match inherited_read_cache_lookup(obj, key) {
        Lookup::Hit(value) => Some(value),
        Lookup::Declined | Lookup::Unknown => None,
    }
}

/// The hit, plus the one other thing the table can say: that a walk from this
/// pair declined and must not be re-run. Only `get_field_ic_miss_impl` cares
/// about the difference, because it is the only caller that would otherwise
/// walk.
///
/// # Safety
/// `obj` is a masked, non-null heap pointer the caller has already established
/// is a plausible heap address; `key` may be null.
#[inline]
pub(crate) unsafe fn inherited_read_cache_lookup(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Lookup {
    if key.is_null() || !cache_enabled() {
        return Lookup::Unknown;
    }
    let addr = obj as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr) {
        return Lookup::Unknown;
    }
    // The receiver's identity word: class id at +0, ShapeId at +4. One load,
    // taken BEFORE the kind is proved, so it must be safe on any
    // pointer-tagged value that can reach here.
    //
    // #10828 proves rule 3 ("no non-object cell holds a live ShapeId at +4")
    // over GC cell KINDS. Three pointer-tagged values are NOT in that table:
    // `SymbolHeader`, `AsyncHookHandle` and `AsyncResourceHandle` are
    // `Box::into_raw` native allocations outside the arena. Checked here
    // because this read reaches them, and because the emitted sequence this
    // cache is being built toward will fold both words into ONE 8-byte load
    // and one compare:
    //
    // * `SymbolHeader` (24 bytes): +0 is `magic` = 0x5359_4D42, +4 is
    //   `registered`, which is 0 or 1. Both reads are in bounds and neither
    //   word can be a live ShapeId, which start at 0x8000_0000. Safe BY
    //   CONSTRUCTION.
    // * `AsyncHookHandle` (8 bytes): the whole payload is `index: usize`, a
    //   Vec index, so +4 is its high half — zero. In bounds, cannot collide.
    //   Safe BY CONSTRUCTION.
    // * `AsyncResourceHandle` (24 bytes): +0 is `ids.async_id: u64`, a
    //   monotonic counter, so +4 is its high half. In bounds, and zero until
    //   a single process creates 2^32 async resources. This is the one of the
    //   three that is safe BY MAGNITUDE rather than by construction, i.e. the
    //   same class of argument #10824 refused for buffer capacities. It is not
    //   load-bearing here — `is_shape_id` below rejects a zero word anyway —
    //   but an emitted guard that drops that test would be resting on it.
    //
    // What actually protects this path is the `is_shape_id` range test inside
    // `object_shape_stamp`: a word outside [0x8000_0000, 0xC000_0000) answers
    // 0 and returns `Unknown` two instructions later. The emitted form keeps
    // the same protection for free, because a site's expected ShapeId is
    // always in that range, so a word that is not cannot match it.
    let recv_class_id = (*obj).class_id;
    let recv_shape = shapes::object_shape_stamp(obj);
    if recv_shape == 0 {
        return Lookup::Unknown;
    }
    let index = entry_index(recv_class_id, recv_shape, key as usize);
    let entry = INHERITED_READ_CACHE.with(|cell| (*cell.get())[index]);
    if entry.key_ptr != key as usize
        || entry.recv_shape != recv_shape
        || entry.recv_class_id != recv_class_id
    {
        return Lookup::Unknown;
    }
    // One load, one compare, whatever the depth of the chain. See the module
    // header: this word covers both the semantic property epoch and every
    // structural mutation of an object marked as somebody's prototype.
    if entry.validity != crate::object::proto_validity::proto_validity() {
        return Lookup::Unknown;
    }
    if entry.slot == NEGATIVE_SLOT {
        if stats_enabled() {
            NEG_SERVED.fetch_add(1, Ordering::Relaxed);
        }
        return Lookup::Declined;
    }
    // Only NOW, once the entry has matched on three identities, is it worth
    // proving the receiver really is an object. A non-object cell's word at
    // +4 is a `capacity` or a `func_ptr` half (design doc rule 3), so the
    // ShapeId compare above is not by itself a proof of kind.
    if crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown {
        return Lookup::Unknown;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header_known_plausible(addr) else {
        return Lookup::Unknown;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return Lookup::Unknown;
    }
    let meta = (*obj).meta;
    let recv_proto_bits = if meta.is_null() { 0 } else { (*meta).prototype };
    if recv_proto_bits != entry.recv_proto_bits {
        return Lookup::Unknown;
    }

    if !receiver_address_facts_ok(meta) {
        return Lookup::Unknown;
    }
    let holder = entry.holder as *const ObjectHeader;
    let field = (holder as *const u8)
        .add(std::mem::size_of::<ObjectHeader>() + entry.slot as usize * 8)
        as *const u64;
    let bits = *field;
    // A deleted holder slot is a `TAG_HOLE`. `delete` bumps the semantic epoch
    // so this is unreachable today; it costs one compare and it is the check
    // that makes the claim not depend on that.
    if bits == crate::value::TAG_HOLE {
        return Lookup::Unknown;
    }
    let value = JSValue::from_bits(bits);
    // `try_data_get_bytes` treats an inherited `undefined`/`null` as a miss at
    // some class edges, and priming refuses such a slot. A later store of
    // `undefined` into the holder's slot can still produce one, so mirror it
    // rather than diverge.
    if value.is_undefined() || value.is_null() {
        return Lookup::Unknown;
    }
    if stats_enabled() {
        HITS.fetch_add(1, Ordering::Relaxed);
    }
    Lookup::Hit(value)
}

// --- the prime --------------------------------------------------------------

/// What a declining walk learned, so the decline can be recorded and the walk
/// not repeated. `armed` means the receiver identity and key are known, which
/// is the minimum a negative entry needs; `value_dependent` means the refusal
/// was caused by a VALUE (an `undefined`/`null`/hole in the holder's slot),
/// which a plain store can change with no shape transition and no epoch bump —
/// the one class of refusal that must NOT be remembered, or `proto.a = 1`
/// after a miss would leave the pair declined for the life of the process.
#[derive(Default)]
struct DeclineNote {
    armed: bool,
    value_dependent: bool,
    key_ptr: usize,
    recv_class_id: u32,
    recv_shape: u32,
    recv_proto_bits: u64,
    holder: usize,
    hops: [usize; MAX_HOPS],
    hop_count: u8,
}

/// Walk `obj`'s prototype chain for `key`, and record the result — the holder
/// and slot when every condition in this module's contract holds, the refusal
/// itself when they do not.
///
/// The caller must already have established that `key` is NOT in `obj`'s own
/// key list — `get_field_ic_miss_impl` has just searched it — so this never
/// duplicates an own-property search.
///
/// Returns the value when the walk resolved it, `None` to leave the caller on
/// its existing path. A `None` is always safe: it is today's behaviour.
///
/// # Safety
/// `obj` is a masked, non-null heap pointer; `key` may be null.
pub(crate) unsafe fn inherited_read_cache_prime(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() || !cache_enabled() {
        return None;
    }
    let mut note = DeclineNote::default();
    let result = inherited_read_cache_walk(obj, key, &mut note);
    if result.is_none() {
        if stats_enabled() {
            DECLINES.fetch_add(1, Ordering::Relaxed);
        }
        if note.armed && !note.value_dependent {
            let entry = Entry {
                key_ptr: note.key_ptr,
                validity: crate::object::proto_validity::proto_validity(),
                recv_proto_bits: note.recv_proto_bits,
                recv_class_id: note.recv_class_id,
                recv_shape: note.recv_shape,
                holder: note.holder,
                hops: note.hops,
                hop_count: note.hop_count,
                slot: NEGATIVE_SLOT,
            };
            let index = entry_index(note.recv_class_id, note.recv_shape, note.key_ptr);
            INHERITED_READ_CACHE.with(|cell| {
                (*cell.get())[index] = entry;
            });
        }
    }
    result
}

/// The walk itself. Every `None` here is a refusal; `note` is what makes the
/// refusal recordable.
///
/// # Safety
/// As [`inherited_read_cache_prime`], whose guards this runs under.
unsafe fn inherited_read_cache_walk(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    note: &mut DeclineNote,
) -> Option<JSValue> {
    let key_addr = key as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(key_addr)
        || !address_is_prime_stable(key_addr)
    {
        return None;
    }
    if (*key).byte_len > (*key).capacity || (*key).byte_len >= 1 << 28 {
        return None;
    }
    let key_bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    // Mirror `native_get::try_data_get_bytes`'s refusals exactly: private
    // members, `constructor` (synthesized per receiver), and non-UTF-8 keys,
    // whose descriptor summaries use a different hash.
    if key_bytes.first() == Some(&b'#')
        || key_bytes == b"constructor"
        || std::str::from_utf8(key_bytes).is_err()
    {
        return None;
    }
    let accessor_bit = 1u64 << (super::key_bytes_hash(key_bytes.as_ptr(), key_bytes.len()) & 63);

    // The caller is `get_field_ic_miss_impl`'s fall-through, which is reached
    // by NON-OBJECT receivers too (`miss_reason` only distinguishes them when
    // IC diagnostics are on). Prove the kind before dereferencing anything:
    // an Array's word at +0 is its `length` and at +4 its `capacity`, so
    // reading them as `class_id` / ShapeId is not a type error the compiler
    // can see.
    let obj_addr = obj as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(obj_addr)
        || crate::arena::classify_heap_generation(obj_addr) == crate::arena::HeapGeneration::Unknown
    {
        return None;
    }
    let recv_header = match crate::value::addr_class::try_read_gc_header_known_plausible(obj_addr) {
        Some(header) => header,
        None => return None,
    };
    if recv_header.obj_type != crate::gc::GC_TYPE_OBJECT
        || recv_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || recv_header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return None;
    }
    match shapes::object_shape_descriptor(obj) {
        Some(shape) if shape.object_kind == shapes::ShapeObjectKind::Ordinary => {}
        _ => return None,
    }
    let recv_class_id = (*obj).class_id;
    let recv_shape = shapes::object_shape_stamp(obj);
    if recv_shape == 0 {
        return None;
    }
    let recv_meta = (*obj).meta;
    if !receiver_address_facts_ok(recv_meta) {
        return None;
    }
    if key_bytes == b"toJSON" && crate::perf_hooks::is_perf_entry_object(obj) {
        return None;
    }
    let recv_proto_bits = if recv_meta.is_null() {
        0
    } else {
        (*recv_meta).prototype
    };
    // From here on the pair (receiver identity, key) is known, so a refusal
    // can be recorded against it. Everything refused ABOVE this line is
    // refused on grounds that are not a function of that pair.
    note.armed = true;
    note.key_ptr = key_addr;
    note.recv_class_id = recv_class_id;
    note.recv_shape = recv_shape;
    note.recv_proto_bits = recv_proto_bits;
    // An accessor or a customized descriptor for this key ON THE RECEIVER
    // means the generic path owns this read.
    if !recv_meta.is_null()
        && ((*recv_meta).accessor_key_bits & accessor_bit != 0
            || (*recv_meta).attr_key_bits & accessor_bit != 0)
    {
        return None;
    }

    let mut hops = [0usize; MAX_HOPS];
    let mut hop_count = 0usize;
    let mut current = obj;
    let mut current_class_id = recv_class_id;
    let mut current_meta = recv_meta;

    loop {
        // Resolve the next prototype the way `try_data_get_bytes` does.
        let next: *const ObjectHeader = if !current_meta.is_null() && (*current_meta).prototype != 0
        {
            let prototype = JSValue::from_bits((*current_meta).prototype);
            if !prototype.is_pointer() {
                return None;
            }
            prototype.as_pointer()
        } else {
            let synthetic = current_class_id >= 0x8000_0000
                && current_class_id
                    < super::NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed);
            if !synthetic {
                // A default builtin prototype may still need lazy
                // construction, and can be replaced through `globalThis`.
                return None;
            }
            if !super::class_decl_prototype_object(current_class_id).is_null() {
                // Declared prototype metadata has its own precedence.
                return None;
            }
            super::class_prototype_object(current_class_id)
        };
        if next.is_null() || next == current || next == obj {
            return None;
        }
        if hop_count == MAX_HOPS {
            return None;
        }
        let next_addr = next as usize;
        if !crate::value::addr_class::is_plausible_heap_addr(next_addr)
            || !address_is_prime_stable(next_addr)
        {
            return None;
        }
        let header = match crate::value::addr_class::try_read_gc_header_known_plausible(next_addr) {
            Some(header) => header,
            None => return None,
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        {
            return None;
        }
        let shape = match shapes::object_shape_descriptor(next) {
            Some(shape) => shape,
            None => return None,
        };
        if shape.object_kind != shapes::ShapeObjectKind::Ordinary {
            return None;
        }
        if shapes::object_shape_stamp(next) == 0 {
            return None;
        }
        let meta = (*next).meta;
        if !receiver_address_facts_ok(meta) {
            return None;
        }
        // The hop must ALREADY be marked as somebody's prototype. An entry
        // through an unmarked prototype is one that a key added to that
        // prototype would not invalidate, so it must not be created.
        //
        // The `[[Prototype]]` install funnel marks, which covers most routes;
        // this is the safety net for the ones it does not, and it is what
        // makes the coverage obligation self-healing rather than a list to
        // keep complete. Mark the hop and ABANDON the walk: marking allocates
        // a meta record, which can move `obj`, `next` and every address in
        // `hops`, so not one of them may be touched afterwards. The next read
        // of this pair finds the hop marked and primes normally.
        //
        // The refusal is deliberately NOT remembered (`note.armed = false`):
        // marking bumps no validity, so a negative entry recorded here would
        // decline the pair for the life of the process — the same trap the
        // value-dependent refusals avoid.
        if meta.is_null()
            || (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE == 0
        {
            note.armed = false;
            crate::object::proto_validity::mark_object_as_prototype(next_addr);
            return None;
        }
        if key_bytes == b"toJSON" && crate::perf_hooks::is_perf_entry_object(next) {
            return None;
        }
        // A clear Bloom bit PROVES no accessor and no customized descriptor
        // for this key on this hop; a collision declines conservatively.
        if !meta.is_null()
            && ((*meta).accessor_key_bits & accessor_bit != 0
                || (*meta).attr_key_bits & accessor_bit != 0)
        {
            return None;
        }

        hops[hop_count] = next_addr;
        hop_count += 1;
        note.holder = next_addr;
        note.hops = hops;
        note.hop_count = hop_count as u8;

        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(slot) =
                super::keys_find_slot_by_bytes_resolved(keys, shape.logical_key_count, key_bytes)
            {
                // Spilled fields are not reachable by the one-load hit path.
                if slot >= shape.live_inline_slot_count {
                    return None;
                }
                let field = (next as *const u8)
                    .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
                    as *const u64;
                let bits = *field;
                if bits == crate::value::TAG_HOLE {
                    note.value_dependent = true;
                    return None;
                }
                let value = JSValue::from_bits(bits);
                if value.is_undefined() || value.is_null() {
                    // `try_data_get_bytes` treats these as a miss on an
                    // inherited read; do not record a claim it would refuse.
                    // A later store can make this slot a real value with no
                    // shape transition, so this refusal is not remembered.
                    note.value_dependent = true;
                    return None;
                }
                let entry = Entry {
                    key_ptr: key_addr,
                    validity: crate::object::proto_validity::proto_validity(),
                    recv_proto_bits,
                    recv_class_id,
                    recv_shape,
                    holder: next_addr,
                    hops,
                    hop_count: hop_count as u8,
                    slot,
                };
                let index = entry_index(recv_class_id, recv_shape, key_addr);
                INHERITED_READ_CACHE.with(|cell| {
                    (*cell.get())[index] = entry;
                });
                if stats_enabled() {
                    PRIMES.fetch_add(1, Ordering::Relaxed);
                }
                return Some(value);
            }
        }

        current = next;
        current_class_id = (*next).class_id;
        current_meta = meta;
    }
}

// --- GC ---------------------------------------------------------------------

/// Root scan. Both the key and every hop are MARKED, not merely rewritten.
///
/// #6759 phase 3 made the transition cache's equivalent slots weak, and was
/// right to: that cache holds 16384 keys arrays and never dereferences one, so
/// rewrite-only plus a death prune loses nothing. This cache is the other
/// case. A hit LOADS `holder + slot`, so a weak slot that the prune has not
/// yet reached — or that a deadness predicate answers conservatively — is a
/// read of recycled memory, and the value it returns is wrong rather than
/// merely stale. Marking makes that unrepresentable.
///
/// The retention it buys is bounded by the table: at most 512 keys and
/// 512 x 4 prototype objects, and a prototype that a program still reads
/// through is reachable from its constructor anyway. The prune below still
/// runs, so an entry whose owner the collector calls dead is dropped rather
/// than kept alive indefinitely by eviction pressure alone.
pub(crate) fn scan_inherited_read_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    INHERITED_READ_CACHE.with(|cell| unsafe {
        for entry in (*cell.get()).iter_mut() {
            if entry.key_ptr == 0 {
                continue;
            }
            visitor.visit_tagged_usize_slot(&mut entry.key_ptr, crate::value::STRING_TAG);
            for i in 0..entry.hop_count as usize {
                visitor.visit_usize_slot(&mut entry.hops[i]);
            }
            // The same object as the last hop, in its own slot so the hit
            // loads it at a fixed offset. Visiting one object through two
            // slots is what every other multi-slot root does; the second visit
            // finds the forwarding record the first one installed.
            visitor.visit_usize_slot(&mut entry.holder);
        }
    });
}

/// Drop entries naming an owner that did not survive.
///
/// Rewrite-only re-keying has no death story on its own: the address of a
/// collected holder stays in the table and a later allocation at that address
/// turns the entry into a false hit that reads a live object's slot for the
/// wrong key. This is the obligation `gc::dead_owner`'s registry exists to
/// make un-forgettable.
pub(crate) fn prune_dead_inherited_cache_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    INHERITED_READ_CACHE.with(|cell| unsafe {
        for entry in (*cell.get()).iter_mut() {
            if entry.key_ptr == 0 {
                continue;
            }
            let mut dead = is_dead_owner(entry.key_ptr);
            for i in 0..entry.hop_count as usize {
                dead |= is_dead_owner(entry.hops[i]);
            }
            dead |= entry.holder != 0 && is_dead_owner(entry.holder);
            if dead {
                *entry = EMPTY_ENTRY;
            }
        }
    });
}

// --- emitted-code entry (for lane 4) ----------------------------------------

/// Hit / prime / decline counts, for a program or a harness that wants to
/// assert the cache is actually being HIT. A cache that primes and then
/// declines every lookup returns the same values the chain walk would and is
/// invisible in a program's output; this is the only thing that tells them
/// apart from outside the runtime. `which`: 0 hits, 1 primes, 2 declines,
/// 3 declines served from a negative entry (i.e. without a chain walk).
/// Arm the counting with `PERRY_INHERITED_IC_STATS`.
#[no_mangle]
pub extern "C" fn js_inherited_read_cache_stats(which: i32) -> f64 {
    match which {
        0 => inherited_read_cache_hits() as f64,
        1 => inherited_read_cache_primes() as f64,
        2 => inherited_read_cache_declines() as f64,
        3 => inherited_read_cache_neg_served() as f64,
        _ => -1.0,
    }
}

/// The hit, as the emitted read sequence would call it: masked receiver
/// pointer plus interned key, NaN-boxed value back, `TAG_HOLE` for a decline
/// (which no ordinary value can be, so the caller branches on one compare).
///
/// # Safety
/// `obj` is a masked heap pointer whose pointer tag the caller established.
#[no_mangle]
pub unsafe extern "C" fn js_inherited_read_cache_hit_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    match inherited_read_cache_hit(obj, key) {
        Some(value) => f64::from_bits(value.bits()),
        None => f64::from_bits(crate::value::TAG_HOLE),
    }
}

#[cfg(test)]
#[path = "inherited_read_cache_tests.rs"]
mod tests;
