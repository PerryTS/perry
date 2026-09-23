//! A store SITE's proof that `[[Set]]` of its key on a class instance creates
//! an own data property — so the next store of the same key on the same kind
//! of receiver can take the shape-transition append directly.
//!
//! # The cost this removes
//!
//! A store that ADDS a key (`this.pos = pos` in a constructor that declares no
//! fields; `this.parse = this.parse.bind(this)` shadowing an inherited method)
//! has to consult the prototype chain: an inherited setter would run instead,
//! and an inherited non-writable property would refuse the write. For a plain
//! object the runtime already skips that walk on a transition-cache hit
//! (`object_set_field_by_name_transition_fast_impl_value`), but it refuses
//! every CLASS instance ("a real user class must retain the full
//! inherited-setter/prototype walk"). So every key-adding store on a class
//! instance runs the full `[[Set]]` — receiver classification, key coercion,
//! the chain verdict, the own-overwrite probe, then the append: measured
//! ~5,600 instructions per store in tsc's `NodeObject` constructor shape, and
//! 1.27 M of them in one `transpileModule`.
//!
//! # What the site holds, and why it is not a copy of anyone's data
//!
//! One [`ChainStoreEntry`] per site, reached through the site's write cache
//! (word [`CHAIN_ENTRY_WORD`]; the emitted code never reads it). It records
//! the answer the authoritative predicate gave for ONE receiver kind:
//!
//! * `proto_id` — the prototype identity the receiver's SHAPE records
//!   (`shapes::object_proto_id`; [[Prototype]] is a shape fact). It names the
//!   chain the verdict was computed over and is read THROUGH the receiver's
//!   shape at hit time. The rest of the shape — the receiver's own layout —
//!   is deliberately not compared: whether the key is own is answered by the
//!   receiver's shape through its transition edge (`(shape, key)` exists
//!   only for a shape that lacks the key), so one entry serves a constructor
//!   site whose receiver changes shape at every statement while keeping its
//!   prototype.
//! * `key` — the interned key the verdict is about.
//! * `validity` / `vtable_gen` — the two words that move when anything the
//!   verdict read changes (below).
//!
//! It never decides anything alone: every use re-reads the receiver's shape,
//! header flags and the two words, and the append itself goes
//! through the transition lane, which re-validates the receiver's shape. A
//! mismatch falls through to the unchanged `[[Set]]`. Deleting every entry
//! costs only speed.
//!
//! # What the verdict depends on, and what invalidates it
//!
//! The verdict is exactly `!class_instance_set_may_intercept(obj, class_id,
//! key)` — the predicate `proxy::ordinary_set_with_receiver` already uses to
//! decide that a class-instance store is a plain `target_set`. Its inputs:
//!
//! 1. a class getter/setter for the key in the class chain
//!    (`class_chain_has_instance_accessor`) — registered in the vtable, so any
//!    change bumps `VTABLE_GEN` ([`ChainStoreEntry::vtable_gen`]);
//! 2. an accessor or non-writable data property for the key on any prototype
//!    OBJECT on the chain, including `Object.prototype` — a descriptor install
//!    or clear, which bumps the semantic property epoch, folded into
//!    `proto_validity` ([`ChainStoreEntry::validity`]);
//! 3. the chain itself: `setPrototypeOf` / `__proto__` anywhere (semantic
//!    epoch), class-prototype-object registration or replacement
//!    (`class_lookup_surface_gen_bump`, which bumps `proto_validity`), and a
//!    Proxy on the chain (the predicate refuses it, so no entry exists);
//! 4. the receiver's own [[Prototype]] — the `proto_id` its shape records,
//!    which every prototype change on the receiver transitions.
//!
//! Per-object facts are not part of the verdict and are re-checked on every
//! hit from the receiver's header and meta record: frozen / sealed /
//! non-extensible, a null `[[Prototype]]`, an `elements` store, the exotic-
//! receiver flag. An own descriptor on the receiver is re-checked per key by
//! the transition lane itself.
//!
//! # GC
//!
//! `key` is a heap reference. It is a STRONG root
//! (`scan_chain_store_roots_mut`): marked, so it cannot die and have its
//! address reused under an entry, and rewritten, so a move is followed. The
//! retention is bounded by the number of store sites that ever primed, and an
//! interned key is long-lived anyway. `proto_id` is a number, not a
//! reference.
//!
//! # Agents
//!
//! Site caches are process-global, but a heap reference belongs to one heap.
//! Entries are created and consulted only on [`crate::agent::PRIMARY_AGENT`];
//! a `perry/thread` worker takes the unchanged `[[Set]]`, so no worker ever
//! reads or writes a primary-heap reference through a site.

use std::sync::atomic::{AtomicU64, Ordering};

/// Word of the per-site write cache that holds this site's entry pointer
/// (0 = none). Past every word emitted code reads: the static-key PIC reads
/// 0..8 (four `[token, slot]` ways), the dynamic-key IC 0..7.
pub(crate) const CHAIN_ENTRY_WORD: usize = 8;

/// One site's verdict. See the module docs for what each field stands for.
#[derive(Clone, Copy)]
pub(crate) struct ChainStoreEntry {
    key: usize,
    proto_id: u64,
    validity: u64,
    vtable_gen: u64,
}

crate::perry_thread_local! {
    /// Every entry this thread created. Entries are never freed: a site keeps
    /// its entry for the life of the process and re-primes it in place.
    static CHAIN_STORE_ENTRIES: std::cell::UnsafeCell<Vec<*mut ChainStoreEntry>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
}

static CHAIN_STORE_HITS: AtomicU64 = AtomicU64::new(0);
static CHAIN_STORE_PRIMES: AtomicU64 = AtomicU64::new(0);
static CHAIN_STORE_REFUSED: AtomicU64 = AtomicU64::new(0);
/// Why a slow store did NOT leave a verdict behind, one bucket per reason, so
/// a site that silently keeps missing says which condition it fails.
/// Indices: 0 key not nameable, 1 layout unchanged, 2 receiver not an
/// eligible class instance, 3 receiver not an ordinary shape, 4 identity
/// changed across the predicate, 5 a prototype hop that is not an ordinary
/// object.
static CHAIN_STORE_SKIPPED: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

#[inline]
fn note_skip(reason: usize) {
    if stats_enabled() {
        CHAIN_STORE_SKIPPED[reason].fetch_add(1, Ordering::Relaxed);
    }
}

/// `PERRY_CHAIN_STORE_IC=0` turns the lane off in a binary that has it (A/B
/// in one build). Both settings store identically.
#[inline]
fn lane_enabled() -> bool {
    static CHAIN_STORE_LANE_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CHAIN_STORE_LANE_ON.get_or_init(|| {
        crate::gc::env_default_on_from_value(std::env::var("PERRY_CHAIN_STORE_IC").ok().as_deref())
    })
}

/// Counting is gated so a default-off diagnostic costs one predictable branch.
/// `PERRY_CHAIN_STORE_IC_STATS=1` prints the three counts at exit.
#[inline]
fn stats_enabled() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    {
        static CHAIN_STORE_STATS_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *CHAIN_STORE_STATS_ON.get_or_init(|| {
            let on = std::env::var_os("PERRY_CHAIN_STORE_IC_STATS").is_some();
            if on {
                extern "C" fn report() {
                    let skipped: Vec<u64> =
                        CHAIN_STORE_SKIPPED.iter().map(|c| c.load(Ordering::Relaxed)).collect();
                    eprintln!(
                        "[chain-store-ic] hits={} primes={} refused={} skipped(key,unchanged,kind,irregular,moved,hop)={:?}",
                        CHAIN_STORE_HITS.load(Ordering::Relaxed),
                        CHAIN_STORE_PRIMES.load(Ordering::Relaxed),
                        CHAIN_STORE_REFUSED.load(Ordering::Relaxed),
                        skipped
                    );
                }
                unsafe { libc::atexit(report) };
            }
            on
        })
    }
}

#[inline]
fn on_primary_agent() -> bool {
    crate::agent::current_agent() == crate::agent::PRIMARY_AGENT
}

/// Header flags that make a receiver ineligible whatever the site recorded.
const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
    | crate::gc::OBJ_FLAG_SEALED
    | crate::gc::OBJ_FLAG_NO_EXTEND
    | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
    | crate::gc::OBJ_FLAG_NULL_PROTO;

/// The prototype identity an entry is keyed on, read THROUGH a live
/// receiver's shape — or `None` when the receiver is not an ordinary class
/// instance this lane may serve. Allocation-free.
///
/// # Safety
/// `obj` is a masked heap pointer the caller established is above the handle
/// band.
#[inline]
unsafe fn receiver_proto_id(obj: *const crate::ObjectHeader) -> Option<u64> {
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & BLOCKING_FLAGS != 0
    {
        return None;
    }
    let class_id = (*obj).class_id;
    // Class id 0 and closed-literal anon shapes are served by the plain-object
    // transition lane already; native-module namespaces own their writes.
    if class_id == 0
        || class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::is_anon_shape_class_id(class_id)
    {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
    {
        return None;
    }
    crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(obj))
}

/// An interned heap-string key this lane can name, or null.
///
/// # Safety
/// `key` is null or a live `StringHeader`.
#[inline]
unsafe fn eligible_key(key: *const crate::StringHeader) -> bool {
    if key.is_null() || !crate::value::addr_class::is_above_handle_band(key as usize) {
        return false;
    }
    let Some(gc) = crate::value::addr_class::try_read_gc_header(key as usize) else {
        return false;
    };
    if gc.obj_type != crate::gc::GC_TYPE_STRING
        || gc.gc_flags & (crate::gc::GC_FLAG_FORWARDED | crate::gc::GC_FLAG_INTERNED)
            != crate::gc::GC_FLAG_INTERNED
    {
        return false;
    }
    let len = (*key).byte_len as usize;
    if len == 0 {
        return false;
    }
    // Private members (`#x`) are not ordinary properties, and a canonical
    // index (digit-leading) can reach Array-subclass / `Object.prototype`
    // index bookkeeping the append does not perform.
    let first = *crate::string::string_data(key);
    first != b'#' && !first.is_ascii_digit()
}

/// The site's entry, if it has one and this thread may use it.
///
/// # Safety
/// `cache_slot` is null or a live write-cache slot.
#[inline]
unsafe fn site_entry(
    cache_slot: *mut crate::proxy::WritePicCacheSlot,
) -> Option<*mut ChainStoreEntry> {
    if cache_slot.is_null() {
        return None;
    }
    let cache = crate::object::pic_slot_peek(cache_slot);
    if cache.is_null() {
        return None;
    }
    let word = (*cache)[CHAIN_ENTRY_WORD] as usize;
    (word != 0).then_some(word as *mut ChainStoreEntry)
}

/// Does this site's verdict cover a store of `key` into `obj` right now?
///
/// Allocation-free; a `true` licenses the caller to append through the
/// transition lane with the chain proof. `false` is always safe.
///
/// # Safety
/// `cache_slot` is null or a live write-cache slot; `obj` is a masked heap
/// pointer above the handle band; `key` is null or a live `StringHeader`.
#[inline]
pub(crate) unsafe fn chain_store_proven(
    cache_slot: *mut crate::proxy::WritePicCacheSlot,
    obj: *const crate::ObjectHeader,
    key: *const crate::StringHeader,
) -> bool {
    if !lane_enabled() {
        return false;
    }
    let Some(entry) = site_entry(cache_slot) else {
        return false;
    };
    let entry = *entry;
    if entry.key != key as usize || !on_primary_agent() {
        return false;
    }
    let Some(proto_id) = receiver_proto_id(obj) else {
        return false;
    };
    proto_id == entry.proto_id
        && entry.validity == crate::object::proto_validity::proto_validity()
        && entry.vtable_gen == crate::object::class_registry::vtable_generation()
}

/// Count a store the lane served.
#[inline]
pub(crate) fn note_chain_store_hit() {
    if stats_enabled() {
        CHAIN_STORE_HITS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record the site's verdict after the full `[[Set]]` has run.
///
/// Called with the receiver and key RE-READ from the caller's roots. The
/// predicate may allocate (it can materialize a class prototype object), so
/// the receiver is rooted across it and its identity is read AFTER it; the
/// two invalidation words are read BEFORE it, so an event that lands during
/// the predicate leaves the entry already stale rather than wrongly current.
///
/// # Safety
/// `cache_slot` is null or a live write-cache slot; `target` and `key` are
/// live values the caller holds rooted.
pub(crate) unsafe fn chain_store_prime(
    cache_slot: *mut crate::proxy::WritePicCacheSlot,
    target: f64,
    key: *const crate::StringHeader,
) {
    if cache_slot.is_null() || !lane_enabled() || !on_primary_agent() {
        return;
    }
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut crate::ObjectHeader;
    if !crate::value::addr_class::is_above_handle_band(obj as usize) || !eligible_key(key) {
        note_skip(0);
        return;
    }
    if receiver_proto_id(obj).is_none() {
        note_skip(2);
        return;
    }
    // A receiver whose [[Prototype]] DIVERGED from its class default — a
    // per-evaluation class (`ClassEvaluation` link: every instance of a class
    // declared inside a function, i.e. tsc's `NodeObject`) or a
    // `setPrototypeOf` — is admitted: its shape names that prototype, so the
    // entry is keyed on the real chain the predicate walked.
    if !crate::object::object_is_regular(obj) {
        note_skip(3);
        return;
    }
    // Receiver and key are rooted as NaN-boxed values and every pointer is
    // re-derived from its root after each call that can allocate (#7528's
    // re-read discipline): the hop marking and the predicate can both
    // allocate, and so move either.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(target);
    let key_h = scope.root_nanbox_f64(f64::from_bits(
        crate::value::JSValue::string_ptr(key as *mut _).bits(),
    ));
    let recv_now = || (recv_h.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    let key_now = || {
        (key_h.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK)
            as *const crate::StringHeader
    };
    // Every object this verdict depends on must be MARKED as a prototype
    // before the two words are read, so that a later accessor, non-writable
    // property or `delete` on any of them moves `proto_validity`
    // (`proto_validity::mutation_owner_may_be_a_recorded_hop`).
    if !mark_chain_hops(&scope, recv_h.get_nanbox_f64()) {
        note_skip(5);
        return;
    }
    let validity = crate::object::proto_validity::proto_validity();
    let vtable_gen = crate::object::class_registry::vtable_generation();
    let class_id = (*(recv_now() as *const crate::ObjectHeader)).class_id;
    let intercepts = crate::object::class_instance_set_may_intercept(
        recv_now(),
        class_id,
        key_h.get_nanbox_f64(),
    );
    if intercepts {
        if stats_enabled() {
            CHAIN_STORE_REFUSED.fetch_add(1, Ordering::Relaxed);
        }
        return;
    }
    let obj = recv_now() as *const crate::ObjectHeader;
    let key = key_now();
    let Some(proto_id) = receiver_proto_id(obj) else {
        note_skip(4);
        return;
    };
    if (*obj).class_id != class_id {
        note_skip(4);
        return;
    }
    let cache = crate::object::pic_slot_resolve(cache_slot);
    let fresh = ChainStoreEntry {
        key: key as usize,
        proto_id,
        validity,
        vtable_gen,
    };
    let word = (*cache)[CHAIN_ENTRY_WORD] as usize;
    if word != 0 {
        *(word as *mut ChainStoreEntry) = fresh;
    } else {
        let entry = Box::into_raw(Box::new(fresh));
        CHAIN_STORE_ENTRIES.with(|cell| (*cell.get()).push(entry));
        (*cache)[CHAIN_ENTRY_WORD] = entry as i64;
    }
    if stats_enabled() {
        CHAIN_STORE_PRIMES.fetch_add(1, Ordering::Relaxed);
    }
}

/// The interned key pointer for a store's key VALUE, without allocating: a
/// heap string that is already interned, or an SSO immediate whose content
/// is in the intern table. `None` for anything else — the caller takes the
/// ordinary path, which interns it, so the next store of the same key finds
/// it.
///
/// # Safety
/// `key` is a live key value.
#[inline]
pub(crate) unsafe fn interned_key_for_store(key: f64) -> Option<*const crate::StringHeader> {
    let bits = key.to_bits();
    let jv = crate::value::JSValue::from_bits(bits);
    if bits & !crate::value::POINTER_MASK == crate::value::STRING_TAG {
        let ptr = (bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
        if eligible_key(ptr) {
            return Some(ptr);
        }
        // A string LITERAL is allocated at module init with
        // `js_string_from_bytes`, not interned, so a key longer than an SSO
        // immediate usually arrives here as a private copy. Its interned twin
        // — the pointer the transition table and every other site use — is a
        // read-only table probe away.
        if !crate::value::addr_class::is_above_handle_band(ptr as usize)
            || !crate::value::addr_class::try_read_gc_header(ptr as usize)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_STRING)
        {
            return None;
        }
        let len = (*ptr).byte_len as usize;
        if len == 0 || len > (*ptr).capacity as usize {
            return None;
        }
        let bytes = std::slice::from_raw_parts(crate::string::string_data(ptr), len);
        let twin = crate::string::intern_lookup_bytes(bytes)?;
        return eligible_key(twin).then_some(twin);
    }
    if jv.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let bytes = crate::string::js_string_key_bytes(jv, &mut buf)?;
        let ptr = crate::string::intern_lookup_bytes(bytes)?;
        return eligible_key(ptr).then_some(ptr);
    }
    None
}

/// The receiver's ShapeId word before a slow store, so the miss handler can
/// tell afterwards whether the store changed the receiver's layout (the one
/// kind of store this lane serves). 0 for anything that is not a heap object.
///
/// # Safety
/// `target` is a live value.
#[inline]
pub(crate) unsafe fn pre_store_shape(target: f64) -> u32 {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return 0;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    if !crate::value::addr_class::is_above_handle_band(obj as usize) {
        return 0;
    }
    match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
            crate::object::shapes::object_shape_stamp(obj)
        }
        _ => 0,
    }
}

/// Serve a key-adding store at a site whose verdict covers this receiver:
/// the transition append with the chain proof. `None` leaves the caller on
/// its path with its operands untouched — an eligible key is already
/// interned, so the lane's only allocation (spill growth) happens on the
/// success path alone.
///
/// # Safety
/// As [`chain_store_proven`]; `target` and `value` are live values.
#[inline]
pub(crate) unsafe fn chain_store_try(
    cache_slot: *mut crate::proxy::WritePicCacheSlot,
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
) -> Option<f64> {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut crate::ObjectHeader;
    if !crate::value::addr_class::is_above_handle_band(obj as usize)
        || !chain_store_proven(cache_slot, obj, key)
    {
        return None;
    }
    let mut refresh = None;
    let stored = crate::object::object_set_field_by_name_transition_chain_proven_value(
        obj,
        key,
        value,
        &mut refresh,
    );
    if stored.is_some() {
        note_chain_store_hit();
    }
    stored
}

/// After a slow store: record the site's verdict if the store changed the
/// receiver's layout and the site does not already hold a verdict covering
/// it. Everything else returns at once, so a site whose stores are own-slot
/// overwrites never pays the predicate.
///
/// # Safety
/// As [`chain_store_prime`].
#[inline]
pub(crate) unsafe fn chain_store_after_miss(
    cache_slot: *mut crate::proxy::WritePicCacheSlot,
    pre_shape: u32,
    target: f64,
    key: *const crate::StringHeader,
) {
    if cache_slot.is_null() || pre_shape == 0 {
        return;
    }
    if key.is_null() {
        note_skip(0);
        return;
    }
    if pre_store_shape(target) == pre_shape {
        note_skip(1);
        return;
    }
    let bits = target.to_bits();
    let obj = (bits & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    if chain_store_proven(cache_slot, obj, key) {
        return;
    }
    chain_store_prime(cache_slot, target, key);
}

/// Walk `obj`'s prototype chain exactly as the verdict's predicate does
/// (`js_object_get_prototype_of`) and mark every hop as a prototype. `false`
/// when a hop is anything but an ordinary heap object — a Proxy, a function
/// object, a handle — or the chain does not end within 64 hops; the caller
/// then records nothing.
///
/// # Safety
/// `receiver` is a live ordinary object the caller holds rooted.
unsafe fn mark_chain_hops(scope: &crate::gc::RuntimeHandleScope, receiver: f64) -> bool {
    let mut hop = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(receiver));
    for _ in 0..64 {
        let bits = hop.get_nanbox_f64().to_bits();
        if bits == crate::value::TAG_NULL {
            return true;
        }
        if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
            return false;
        }
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        if !crate::value::addr_class::is_above_handle_band(addr)
            || !crate::value::addr_class::try_read_gc_header(addr)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
        {
            return false;
        }
        if !crate::object::proto_validity::object_is_marked_prototype(addr) {
            let _ = crate::object::proto_validity::mark_object_as_prototype(addr);
        }
        let next = crate::object::js_object_get_prototype_of(hop.get_nanbox_f64());
        hop = scope.root_nanbox_f64(next);
    }
    false
}

/// Root scan: every entry's key is a STRONG root — marked and rewritten. See
/// the module docs.
pub(crate) fn scan_chain_store_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CHAIN_STORE_ENTRIES.with(|cell| unsafe {
        for &entry in (*cell.get()).iter() {
            let entry = &mut *entry;
            if entry.key != 0 {
                visitor.visit_tagged_usize_slot(&mut entry.key, crate::value::STRING_TAG);
            }
        }
    });
}

#[cfg(test)]
#[path = "chain_store_tests.rs"]
mod tests;
