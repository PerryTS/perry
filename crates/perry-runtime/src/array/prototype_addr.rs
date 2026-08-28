//! The memoized `Array.prototype` / `Object.prototype` addresses (#6981),
//! **one pair per thread** (#7988).
//!
//! Two `usize` cells and the algebra over them: lazy resolution from
//! `globalThis`, healing through the GC forwarding chain, and the registered
//! root scanner that lets a relocating cycle rewrite them.
//!
//! Split out of `indexing.rs` because it is one subject with one invariant —
//! *every cell an accessor reads is a cell the collector rewrites* — and that
//! invariant is easiest to keep true when the cells, the accessors and the
//! scanner are the only things in the file (and it kept `indexing.rs` under
//! `scripts/check_file_size.sh`'s 2000-line cap).
//!
//! # Why the cells are PER-THREAD (#7988)
//!
//! Both cells hold a **raw address of an object in a thread-local arena**, and
//! the realm they name is per-thread: `js_get_global_this` bootstraps
//! `THREAD_GLOBAL_THIS` once *per thread*, so every `perry/thread` agent has
//! its own `Array.prototype` and its own `Object.prototype` in its own heap.
//! When the cells were process-global `AtomicUsize` statics they missed only
//! once *per process*, so the first thread to touch either intrinsic decided
//! the value for every other agent, with three consequences:
//!
//!   1. **Wrong identity.** [`object_prototype_addr_matches`] on agent B
//!      compared B's objects against *A's* `Object.prototype`, so B's own
//!      intrinsic was never recognised. `array_oob_prototype_get`'s hole/OOB
//!      fallbacks and the typed-feedback guards that consult it silently took
//!      the other branch on every thread but the first — B's
//!      `Object.prototype[7] = v` never even flipped `OBJECT_PROTO_HAS_INDEX`,
//!      because the write hook's "is this the prototype?" test compared against
//!      a foreign address.
//!   2. **Unattributed dereference.** [`heal_prototype_addr`] reads the cached
//!      address's `GcHeader` from *any* thread, and `note_array_index_write`
//!      calls it on every indexed array write. A's collector can sweep or move
//!      that object, and A's arena blocks are `dealloc`'d at thread exit, so
//!      the read was on memory the reading thread had no claim to.
//!   3. **Cross-thread root rewrite.** [`scan_prototype_addr_cache_roots_mut`]
//!      is registered per thread and *writes* the cell with **its own**
//!      to-space address, so agent B's collector could overwrite a cell naming
//!      A's heap with a B-heap address.
//!
//! All three are structural once the storage is per-thread: a cell only ever
//! holds an address this thread allocated, healed by this thread's forwarding
//! chain and rewritten by this thread's collector.
//!
//! # Why a thread-local is affordable here
//!
//! [`crate::perry_thread_local`] is not `std::thread_local!`: the address of
//! the value lands in this thread's [`crate::tls_hot::HotTls`] cache, which on
//! Apple aarch64 is reached with an `mrs` plus two loads that LLVM CSEs across
//! the enclosing function — not the out-of-line `_tlv_get_addr` call that made
//! "Darwin has no local-exec TLS" the recorded objection to this fix (#7955).
//! Both intrinsics share ONE declaration (an array indexed by
//! [`ARRAY_PROTO_CACHE`] / [`OBJECT_PROTO_CACHE`]) so a function that consults
//! both — `array_oob_prototype_get` does — pays one resolution, not two.

use std::cell::Cell;

/// How many intrinsic prototype addresses one realm memoizes.
///
/// This is the length of BOTH the per-thread cell array and the builtin-name
/// table below, and the root scanner iterates the cell array itself, so
/// "a cell an accessor reads that the collector never rewrites" — the #6981
/// defect — is not representable. Adding a third memoized intrinsic address
/// means bumping this and adding a row to [`PROTOTYPE_ADDR_BUILTINS`]; it is
/// then covered by both halves automatically.
const PROTOTYPE_ADDR_CACHE_COUNT: usize = crate::tls_hot::INLINE_PROTOTYPE_ADDR_ROWS;

/// Row index of the `Array.prototype` cell.
const ARRAY_PROTO_CACHE: usize = 0;
/// Row index of the `Object.prototype` cell.
const OBJECT_PROTO_CACHE: usize = 1;

/// **THIS THREAD's** lazily-memoized intrinsic prototype addresses, indexed
/// by [`ARRAY_PROTO_CACHE`] / [`OBJECT_PROTO_CACHE`]. `usize::MAX` marks a
/// row as not-yet-computed.
///
/// Row 0 is `Array.prototype`. An out-of-bounds element read on an ordinary
/// array must fall through to `Array.prototype[index]` (ECMA-262
/// OrdinaryGet → prototype chain), but in real code nobody adds numeric
/// indices to `Array.prototype`, so the hot OOB path stays one load until
/// the (rare) write flips `ARRAY_PROTO_HAS_INDEX`.
///
/// Row 1 is `Object.prototype`: a numeric index installed there
/// (`Object.prototype[2] = 2`, or a defineProperty accessor) shows through
/// array HOLES and OOB reads (chain: arr → Array.prototype →
/// Object.prototype; test262 concat/S15.4.4.4_A3_T3). Consulted by the
/// typed-feedback guards and the hole/OOB read fallbacks.
///
/// ***THESE ARE RAW ADDRESSES OF MOVABLE OBJECTS*** (#6981).
/// `Array.prototype` relocates two different ways, and BOTH leave the cache
/// pointing at a `GC_FLAG_FORWARDED` stub while every reader resolves its
/// own receiver through `clean_arr_ptr` (which follows forwarding):
///
///   1. `js_array_grow` — an indexed write past the dense capacity
///      (`Array.prototype[300] = v`) reallocates and forwards the old head;
///   2. the copying young-gen minor — it evacuates the prototype and
///      forwards.
///
/// A stale cache is not merely a wrong value: `array_oob_prototype_get`'s
/// self-recursion guard is `proto != receiver`, and after a move those are
/// two different addresses **for the same object**, so the guard stops
/// firing and `js_array_get_f64` ⇄ `array_oob_prototype_get` recurse until
/// the stack guard page (SIGSEGV, "excessive recursion"). Hence the two
/// defences below: [`memoized_prototype_addr`] resolves the forwarding
/// chain and self-heals, and [`scan_prototype_addr_cache_roots_mut`] lets
/// the collector rewrite the slot so the address stays live even once the
/// from-space stub is recycled.
///
/// The rows live INLINE in this thread's [`crate::tls_hot::HotTls`]: they are
/// consulted on every indexed array write, and a generic hot slot cost one
/// more dependent load than the value itself.
#[inline(always)]
fn prototype_addrs() -> &'static [Cell<usize>; PROTOTYPE_ADDR_CACHE_COUNT] {
    &crate::tls_hot::hot().prototype_addrs
}

/// The `globalThis` builtin whose `.prototype` fills each row of
/// [`PROTOTYPE_ADDRS`], in the same order.
///
/// The pairing is POSITIONAL rather than two hand-written accessors so that the
/// second fact a reader has to trust — "each accessor resolves the builtin its
/// cell is named for" — is established by construction instead of by a test
/// that has to mutate a process-global to observe it (#7955).
static PROTOTYPE_ADDR_BUILTINS: [&[u8]; PROTOTYPE_ADDR_CACHE_COUNT] = [b"Array", b"Object"];

/// GC root scanner for this thread's memoized prototype addresses (#6981).
///
/// The cells hold raw addresses of movable objects, so a relocating cycle must
/// REWRITE them exactly like the other address-holding side tables
/// (`CLASS_PROTOTYPE_OBJECTS`, `TYPED_ARRAY_VIEW_META`, …). Forwarding-chain
/// healing alone is not sufficient: once the from-space stub is swept and its
/// block recycled the `GC_FLAG_FORWARDED` bit is gone, and the cache would then
/// name an unrelated live object. Both intrinsics are reachable from
/// `globalThis`, so the marking half of this visit is redundant; the rewriting
/// half is the point.
///
/// #7988: the scanner runs on the collecting thread and now visits **that
/// thread's** cells. Before the storage was per-thread it wrote the collecting
/// thread's to-space address into a cell that could be naming another agent's
/// heap.
pub fn scan_prototype_addr_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    for cell in prototype_addrs() {
        rewrite_prototype_addr_slot(cell, visitor);
    }
}

/// The per-cell half of [`scan_prototype_addr_cache_roots_mut`].
///
/// Split out so the #6981 rewrite algebra can be exercised on a cell the test
/// owns privately: driving it through the realm's real intrinsics made the
/// assertion depend on no other libtest thread touching them meanwhile, which
/// is the #7955 flake.
fn rewrite_prototype_addr_slot(
    cache: &Cell<usize>,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    let cached = cache.get();
    if cached == usize::MAX || cached == 0 {
        return;
    }
    let mut addr = cached;
    if visitor.visit_usize_slot(&mut addr) {
        // GC_STORE_AUDIT(ROOT): this IS the collector's root-rewrite of a
        // registered side-table slot, running inside a root scan with the
        // mutator stopped. `visit_usize_slot` returns true only when it
        // relocated the object, and the value written is the visitor's own
        // to-space address — barriering it would push an edge into the
        // remembered set that this very cycle is rebuilding.
        cache.set(addr);
    }
}

/// Read a memoized prototype address, resolving it through the GC forwarding
/// chain. `None` means "not resolved yet" — the caller must run the
/// `globalThis` bootstrap.
#[inline]
fn memoized_prototype_addr(cache: &Cell<usize>) -> Option<usize> {
    let cached = cache.get();
    (cached != usize::MAX).then(|| heal_prototype_addr(cache, cached))
}

/// Re-read a memoized prototype address through the GC forwarding chain and
/// write the healed address back, so every caller compares (and dereferences)
/// the object's CURRENT location. See the [`PROTOTYPE_ADDRS`] doc for why an
/// unresolved cache is a hang, not just a wrong answer (#6981).
///
/// `note_array_index_write` calls this on every indexed array write until the
/// prototype is polluted, so the not-forwarded case must stay call-free: the
/// `try_read_gc_header` probe is `#[inline(always)]` and reduces to two range
/// compares plus one load of a `gc_flags` byte at a fixed, permanently-hot
/// address. It also classifies the address band before dereferencing, so the
/// not-yet-resolved sentinel (`usize::MAX`) and any non-heap value fall
/// straight through.
///
/// #7988: the address is now always one THIS thread allocated, so the header
/// read has an owner. It used to be able to land on another agent's arena
/// block — possibly already swept, moved, or `dealloc`'d at that thread's exit
/// — and a stray `GC_FLAG_FORWARDED` byte there sent `resolve_forwarding` one
/// word further into it.
#[inline]
fn heal_prototype_addr(cache: &Cell<usize>, cached: usize) -> usize {
    let forwarded = unsafe {
        crate::value::addr_class::try_read_gc_header(cached)
            .is_some_and(|header| header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0)
    };
    if !forwarded {
        return cached;
    }
    let resolved = crate::value::resolve_forwarding(cached);
    if resolved != cached {
        cache.set(resolved);
    }
    resolved
}

/// Resolve one row of [`PROTOTYPE_ADDRS`]: the memoized address if this thread
/// already knows it (healed through any forwarding chain), otherwise the
/// `globalThis` bootstrap, memoized.
#[inline]
fn resolve_prototype_addr(slot: usize) -> usize {
    if let Some(addr) = memoized_prototype_addr(&prototype_addrs()[slot]) {
        return addr;
    }
    bootstrap_prototype_addr(slot)
}

/// The cold half of [`resolve_prototype_addr`]: derive the intrinsic's address
/// from THIS thread's `globalThis` and memoize it.
///
/// Out of line so the hot arm — `note_array_index_write` on every indexed array
/// write — is a slot load, a compare and a branch, with the whole `globalThis`
/// walk off the fast path.
#[cold]
#[inline(never)]
fn bootstrap_prototype_addr(slot: usize) -> usize {
    let builtin = PROTOTYPE_ADDR_BUILTINS[slot];
    let ctor = crate::object::js_get_global_this_builtin_value(builtin.as_ptr(), builtin.len());
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    let addr = if ctor_value.is_pointer() {
        let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
        if proto_value.is_pointer() {
            proto_value.as_pointer::<u8>() as usize
        } else {
            0
        }
    } else {
        0
    };
    // Don't poison the cache with 0: during runtime init the global constructor
    // may not be materialized yet (symbol writes on other builtin prototypes
    // call into here via `note_array_proto_iterator_write`). Re-derive until it
    // resolves.
    if addr != 0 {
        prototype_addrs()[slot].set(addr);
    }
    addr
}

pub(crate) fn array_prototype_addr() -> usize {
    resolve_prototype_addr(ARRAY_PROTO_CACHE)
}

pub(crate) fn object_prototype_addr() -> usize {
    resolve_prototype_addr(OBJECT_PROTO_CACHE)
}

/// `true` when `addr` is **this realm's** canonical `Object.prototype` (cheap:
/// one slot load + compare; lazily computes the address on first use).
pub(crate) fn object_prototype_addr_matches(addr: usize) -> bool {
    addr != 0 && addr == object_prototype_addr()
}

/// Test-only handle on the shipped wiring, for the read-only assertion in
/// `gc::tests::runtime_roots::prototype_addr_cache`. Returns each accessor's
/// row index paired with the `globalThis` builtin that row bootstraps from.
/// The mutating #6981 cases run on cells they own; nothing hands out a writable
/// reference to the realm's real intrinsic cells (#7955).
#[cfg(test)]
pub(crate) fn test_prototype_addr_cache_wiring() -> [(usize, &'static [u8]); 2] {
    [
        (
            ARRAY_PROTO_CACHE,
            PROTOTYPE_ADDR_BUILTINS[ARRAY_PROTO_CACHE],
        ),
        (
            OBJECT_PROTO_CACHE,
            PROTOTYPE_ADDR_BUILTINS[OBJECT_PROTO_CACHE],
        ),
    ]
}

/// Number of cells this thread owns — i.e. how many
/// [`scan_prototype_addr_cache_roots_mut`] visits. Read by the wiring test so
/// "the scanner visits every cell an accessor can index" is asserted rather
/// than assumed.
#[cfg(test)]
pub(crate) fn test_prototype_addr_cell_count() -> usize {
    prototype_addrs().len()
}

/// The two halves of the #6981 defences, exported so the tests can drive them
/// on a private cell instead of this thread's real ones (#7955).
#[cfg(test)]
pub(crate) fn test_memoized_prototype_addr(cache: &Cell<usize>) -> Option<usize> {
    memoized_prototype_addr(cache)
}

#[cfg(test)]
pub(crate) fn test_rewrite_prototype_addr_slot(
    cache: &Cell<usize>,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    rewrite_prototype_addr_slot(cache, visitor)
}
