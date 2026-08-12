//! The memoized `Array.prototype` / `Object.prototype` addresses (#6981).
//!
//! Two `AtomicUsize` cells and the algebra over them: lazy resolution from
//! `globalThis`, healing through the GC forwarding chain, and the registered
//! root scanner that lets a relocating cycle rewrite them.
//!
//! Split out of `indexing.rs` because it is one subject with one invariant —
//! *every cell an accessor reads is a cell the collector rewrites* — and that
//! invariant is easiest to keep true when the cells, the accessors and the
//! scanner are the only things in the file (and it kept `indexing.rs` under
//! `scripts/check_file_size.sh`'s 2000-line cap).

use std::sync::atomic::{AtomicUsize, Ordering};

/// Lazily-memoized address of the `Array.prototype` array. An out-of-bounds
/// element read on an ordinary array must fall through to
/// `Array.prototype[index]` (ECMA-262 OrdinaryGet → prototype chain), but in
/// real code nobody adds numeric indices to `Array.prototype`, so the hot OOB
/// path stays a single relaxed atomic load until the (rare) write flips
/// `ARRAY_PROTO_HAS_INDEX`. `usize::MAX` marks the address as not-yet-computed.
///
/// ***THIS IS A RAW ADDRESS OF A MOVABLE OBJECT*** (#6981). `Array.prototype`
/// relocates two different ways, and BOTH leave this cache pointing at a
/// `GC_FLAG_FORWARDED` stub while every reader resolves its own receiver
/// through `clean_arr_ptr` (which follows forwarding):
///
///   1. `js_array_grow` — an indexed write past the dense capacity
///      (`Array.prototype[300] = v`) reallocates and forwards the old head;
///   2. the copying young-gen minor — it evacuates the prototype and forwards.
///
/// A stale cache is not merely a wrong value: `array_oob_prototype_get`'s
/// self-recursion guard is `proto != receiver`, and after a move those are two
/// different addresses **for the same object**, so the guard stops firing and
/// `js_array_get_f64` ⇄ `array_oob_prototype_get` recurse until the stack guard
/// page (SIGSEGV, "excessive recursion"). Hence the two defences below:
/// [`memoized_prototype_addr`] resolves the forwarding chain and self-heals,
/// and [`scan_prototype_addr_cache_roots_mut`] lets the collector rewrite the
/// slot so the address stays live even once the from-space stub is recycled.
static ARRAY_PROTO_ADDR: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Same idea for `Object.prototype`: a numeric index installed there
/// (`Object.prototype[2] = 2`, or a defineProperty accessor) shows through
/// array HOLES and OOB reads (chain: arr → Array.prototype →
/// Object.prototype; test262 concat/S15.4.4.4_A3_T3). Consulted by the
/// typed-feedback guards and the hole/OOB read fallbacks.
static OBJECT_PROTO_ADDR: AtomicUsize = AtomicUsize::new(usize::MAX);

/// One memoized intrinsic-prototype address: the cell, and the `globalThis`
/// builtin whose `.prototype` fills it.
///
/// The pairing is a TABLE rather than two hand-written accessors so that the
/// two facts a reader has to trust — "the collector rewrites every cell some
/// accessor reads" and "each accessor resolves the builtin its cell is named
/// for" — are established by construction instead of by a test that has to
/// mutate a process-global to observe them (#7955). See
/// [`PROTOTYPE_ADDR_CACHES`].
struct PrototypeAddrCache {
    cell: &'static AtomicUsize,
    /// `globalThis` key whose `.prototype` this cell memoizes.
    builtin: &'static [u8],
}

/// Every memoized prototype address in the runtime, in the order the GC root
/// scanner visits them.
///
/// [`scan_prototype_addr_cache_roots_mut`] iterates this table and
/// [`array_prototype_addr`] / [`object_prototype_addr`] index it, so a cell
/// that an accessor reads but the collector never rewrites — the #6981 defect
/// — is not representable. Adding a third memoized intrinsic address means
/// adding a row here, and it is covered by both halves automatically.
static PROTOTYPE_ADDR_CACHES: [PrototypeAddrCache; 2] = [
    PrototypeAddrCache {
        cell: &ARRAY_PROTO_ADDR,
        builtin: b"Array",
    },
    PrototypeAddrCache {
        cell: &OBJECT_PROTO_ADDR,
        builtin: b"Object",
    },
];

const ARRAY_PROTO_CACHE: usize = 0;
const OBJECT_PROTO_CACHE: usize = 1;

/// GC root scanner for the memoized prototype addresses (#6981).
///
/// `ARRAY_PROTO_ADDR` / `OBJECT_PROTO_ADDR` hold raw addresses of movable
/// objects, so a relocating cycle must REWRITE them exactly like the other
/// address-holding side tables (`CLASS_PROTOTYPE_OBJECTS`,
/// `TYPED_ARRAY_VIEW_META`, …). Forwarding-chain healing alone is not
/// sufficient: once the from-space stub is swept and its block recycled the
/// `GC_FLAG_FORWARDED` bit is gone, and the cache would then name an unrelated
/// live object. Both intrinsics are reachable from `globalThis`, so the marking
/// half of this visit is redundant; the rewriting half is the point.
pub fn scan_prototype_addr_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    for entry in &PROTOTYPE_ADDR_CACHES {
        rewrite_prototype_addr_slot(entry.cell, visitor);
    }
}

/// The per-cell half of [`scan_prototype_addr_cache_roots_mut`].
///
/// Split out so the #6981 rewrite algebra can be exercised on a cell the test
/// owns privately: driving it through the shipped `static`s made the assertion
/// depend on no other libtest thread touching the realm's real intrinsics
/// meanwhile, which is the #7955 flake.
fn rewrite_prototype_addr_slot(
    cache: &AtomicUsize,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    let cached = cache.load(Ordering::Relaxed);
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
        cache.store(addr, Ordering::Relaxed);
    }
}

/// Read a memoized prototype address, resolving it through the GC forwarding
/// chain. `None` means "not resolved yet" — the caller must run the
/// `globalThis` bootstrap.
#[inline]
fn memoized_prototype_addr(cache: &AtomicUsize) -> Option<usize> {
    let cached = cache.load(Ordering::Relaxed);
    (cached != usize::MAX).then(|| heal_prototype_addr(cache, cached))
}

/// Re-read a memoized prototype address through the GC forwarding chain and
/// write the healed address back, so every caller compares (and dereferences)
/// the object's CURRENT location. See the [`ARRAY_PROTO_ADDR`] doc for why an
/// unresolved cache is a hang, not just a wrong answer (#6981).
///
/// `note_array_index_write` calls this on every indexed array write until the
/// prototype is polluted, so the not-forwarded case must stay call-free: the
/// `try_read_gc_header` probe is `#[inline(always)]` and reduces to two range
/// compares plus one load of a `gc_flags` byte at a fixed, permanently-hot
/// address. It also classifies the address band before dereferencing, so the
/// not-yet-resolved sentinel (`usize::MAX`) and any non-heap value fall
/// straight through.
#[inline]
fn heal_prototype_addr(cache: &AtomicUsize, cached: usize) -> usize {
    let forwarded = unsafe {
        crate::value::addr_class::try_read_gc_header(cached)
            .is_some_and(|header| header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0)
    };
    if !forwarded {
        return cached;
    }
    let resolved = crate::value::resolve_forwarding(cached);
    if resolved != cached {
        cache.store(resolved, Ordering::Relaxed);
    }
    resolved
}

/// Resolve one row of [`PROTOTYPE_ADDR_CACHES`]: the memoized address if it is
/// already known (healed through any forwarding chain), otherwise the
/// `globalThis` bootstrap, memoized.
fn resolve_prototype_addr(entry: &PrototypeAddrCache) -> usize {
    if let Some(addr) = memoized_prototype_addr(entry.cell) {
        return addr;
    }
    let ctor = crate::object::js_get_global_this_builtin_value(
        entry.builtin.as_ptr(),
        entry.builtin.len(),
    );
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
        entry.cell.store(addr, Ordering::Relaxed);
    }
    addr
}

pub(crate) fn array_prototype_addr() -> usize {
    resolve_prototype_addr(&PROTOTYPE_ADDR_CACHES[ARRAY_PROTO_CACHE])
}

pub(crate) fn object_prototype_addr() -> usize {
    resolve_prototype_addr(&PROTOTYPE_ADDR_CACHES[OBJECT_PROTO_CACHE])
}

/// `true` when `addr` is the canonical `Object.prototype` (cheap: cached
/// atomic + compare; lazily computes the address on first use).
pub(crate) fn object_prototype_addr_matches(addr: usize) -> bool {
    addr != 0 && addr == object_prototype_addr()
}

/// Test-only handle on the shipped table, for the read-only wiring assertion
/// in `gc::tests::runtime_roots::prototype_addr_cache`. The mutating #6981
/// cases run on cells they own; nothing hands out a writable reference to the
/// realm's real intrinsic cells any more (#7955).
#[cfg(test)]
pub(crate) fn test_prototype_addr_cache_wiring() -> [(&'static AtomicUsize, &'static [u8]); 2] {
    [
        (
            PROTOTYPE_ADDR_CACHES[ARRAY_PROTO_CACHE].cell,
            PROTOTYPE_ADDR_CACHES[ARRAY_PROTO_CACHE].builtin,
        ),
        (
            PROTOTYPE_ADDR_CACHES[OBJECT_PROTO_CACHE].cell,
            PROTOTYPE_ADDR_CACHES[OBJECT_PROTO_CACHE].builtin,
        ),
    ]
}

/// The two halves of the #6981 defences, exported so the tests can drive them
/// on a private cell instead of the process-global one (#7955).
#[cfg(test)]
pub(crate) fn test_memoized_prototype_addr(cache: &AtomicUsize) -> Option<usize> {
    memoized_prototype_addr(cache)
}

#[cfg(test)]
pub(crate) fn test_rewrite_prototype_addr_slot(
    cache: &AtomicUsize,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    rewrite_prototype_addr_slot(cache, visitor)
}
