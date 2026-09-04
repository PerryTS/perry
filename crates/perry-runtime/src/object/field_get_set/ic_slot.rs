//! #9708: per-site inline caches allocated on first miss, not per emitted site.
//!
//! Codegen used to emit every inline-cache site as its own
//! `[PIC_CACHE_WORDS x i64] zeroinitializer` global — 96 B per property-access
//! site, whether or not the program ever executes it. On a large bundle that
//! is hundreds of thousands of sites and tens of megabytes of `__bss` that
//! turn into dirty resident pages as soon as a neighbour on the same page is
//! touched: the `cc` build carried 262k caches (25 MB) and dirtied 18.7 MB of
//! them at idle while running a few thousand distinct hot sites.
//!
//! Each site now owns an 8-byte **slot** — `@perry_ic_N = private global ptr
//! null` — and the cache words live in a runtime-owned arena, allocated the
//! first time the site's miss handler runs. A site that never executes costs
//! its 8 bytes of zero-fill and nothing else; a site that does costs the same
//! `PIC_CACHE_WORDS` words it always did, packed next to the other caches
//! that were touched around the same time.
//!
//! The emitted hit path loads the slot, folds `!= null` into the receiver
//! guard it already evaluates, and reads the cache words through the loaded
//! pointer; the miss path hands the *slot* to the runtime, which resolves or
//! allocates the cache here. Only the words the runtime writes ever live in
//! the arena, so a cache's layout and every prime/evict policy are unchanged.
//!
//! Publication is a compare-and-swap on the slot so two `perry/thread` agents
//! missing the same site for the first time agree on one cache; the loser's
//! block stays in the arena unused (bounded by the number of such races, and
//! counted). The arena is never freed: the sites reference their caches for
//! the life of the process, exactly as the globals were.

use std::alloc::{alloc_zeroed, handle_alloc_error, Layout};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Bytes requested from the system allocator per arena refill. Holds 682
/// twelve-word caches; a program that primes fewer sites than that pays one
/// zeroed allocation in total.
const PIC_ARENA_CHUNK_BYTES: usize = 64 * 1024;

/// Every cache starts on a 32-byte boundary, so the four words the MRU hit
/// path and the way-state gate read (`tok0`, `slot0`, scratch, state) never
/// straddle a 64-byte line — the property the pre-#9708 layout relied on
/// when it parked the gate at word 3 (see `PIC_WAY_BASE`'s doc).
const PIC_CACHE_ALIGN: usize = 32;

/// Bump-pointer state over the current chunk. Addresses are kept as `usize`
/// on purpose: the arena hands out cache words (tokens and slot indices), it
/// never holds a GC heap pointer, and `usize` keeps the static `Send` without
/// an `unsafe impl`.
struct PicArena {
    cur: usize,
    end: usize,
}

static PIC_ARENA: Mutex<PicArena> = Mutex::new(PicArena { cur: 0, end: 0 });

/// Caches published into a slot (one per site that has missed at least once).
static PIC_SLOTS_RESOLVED: AtomicUsize = AtomicUsize::new(0);
/// Bytes requested from the system allocator for arena chunks.
static PIC_ARENA_BYTES: AtomicUsize = AtomicUsize::new(0);
/// Allocations that lost the publication race to another thread.
static PIC_SLOT_RACES: AtomicUsize = AtomicUsize::new(0);

/// Bump-allocate `bytes` of zeroed, `PIC_CACHE_ALIGN`-aligned memory.
fn pic_arena_alloc(bytes: usize) -> *mut u8 {
    let bytes = bytes.div_ceil(PIC_CACHE_ALIGN) * PIC_CACHE_ALIGN;
    debug_assert!(bytes <= PIC_ARENA_CHUNK_BYTES);
    let mut arena = PIC_ARENA.lock().unwrap_or_else(|e| e.into_inner());
    if arena.cur + bytes > arena.end {
        // SAFETY: the layout is non-zero-sized and its alignment is a power of
        // two; a zeroed chunk is exactly the initial state every cache expects.
        let layout = Layout::from_size_align(PIC_ARENA_CHUNK_BYTES, 64)
            .expect("PIC arena chunk layout is a constant");
        let chunk = unsafe { alloc_zeroed(layout) };
        if chunk.is_null() {
            handle_alloc_error(layout);
        }
        PIC_ARENA_BYTES.fetch_add(PIC_ARENA_CHUNK_BYTES, Ordering::Relaxed);
        arena.cur = chunk as usize;
        arena.end = arena.cur + PIC_ARENA_CHUNK_BYTES;
    }
    let out = arena.cur;
    arena.cur += bytes;
    out as *mut u8
}

/// Resolve a per-site cache slot to its cache, allocating a zeroed one and
/// publishing it into the slot on the first miss.
///
/// `slot` is the address of the emitted `@perry_ic_N` pointer global (or a
/// stack `*mut T` in tests); `T` is the cache's word array — `PicCache` for a
/// property read, the eight-word write cache, the four-word Symbol cache —
/// and decides how many bytes the arena hands out. A null `slot` resolves to
/// a null cache: the write PIC's poly tail passes that to mean "run `[[Set]]`
/// and prime nothing" and every consumer already checks for it.
///
/// # Safety
/// `slot` must be null or point at a live, 8-byte-aligned pointer-sized
/// location that holds either null or a cache previously returned by this
/// function (or any live `T`). All-zero bytes must be a valid `T`.
#[inline]
pub unsafe fn pic_slot_resolve<T>(slot: *mut *mut T) -> *mut T {
    if slot.is_null() {
        return null_mut();
    }
    let atomic = &*(slot as *const AtomicPtr<T>);
    let cur = atomic.load(Ordering::Acquire);
    if !cur.is_null() {
        return cur;
    }
    pic_slot_publish(atomic)
}

/// The cache `slot` currently holds, without allocating: null for a site
/// that has never primed. Runtime entries that only *read* a site's cache
/// (the outlined write IC's hit check, a Symbol miss's epoch reset) go
/// through here so a concurrent first prime on another thread is an atomic
/// publication rather than a torn read.
///
/// # Safety
/// `slot` must be null or a live cache slot (see [`pic_slot_resolve`]).
#[inline]
pub unsafe fn pic_slot_peek<T>(slot: *mut *mut T) -> *mut T {
    if slot.is_null() {
        return null_mut();
    }
    (*(slot as *const AtomicPtr<T>)).load(Ordering::Acquire)
}

#[cold]
#[inline(never)]
unsafe fn pic_slot_publish<T>(atomic: &AtomicPtr<T>) -> *mut T {
    let fresh = pic_arena_alloc(std::mem::size_of::<T>()) as *mut T;
    match atomic.compare_exchange(null_mut(), fresh, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => {
            PIC_SLOTS_RESOLVED.fetch_add(1, Ordering::Relaxed);
            fresh
        }
        Err(existing) => {
            PIC_SLOT_RACES.fetch_add(1, Ordering::Relaxed);
            existing
        }
    }
}

/// Number of sites whose slot holds an arena cache.
pub fn pic_slots_resolved() -> usize {
    PIC_SLOTS_RESOLVED.load(Ordering::Relaxed)
}

/// Bytes the arena has requested from the system allocator.
pub fn pic_arena_bytes() -> usize {
    PIC_ARENA_BYTES.load(Ordering::Relaxed)
}

/// `PERRY_GC_CENSUS`: the lazily allocated inline caches as one side-table
/// row — `entries` is the number of resolved sites, `bytes` the arena's
/// footprint (chunk granularity, so it over-reports by at most one chunk).
pub(crate) fn pic_slot_census() -> Vec<crate::gc::census::SideTableRow> {
    vec![("ic.lazy_caches", pic_slots_resolved(), pic_arena_bytes())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_slot_resolves_to_null_cache() {
        let cache = unsafe { pic_slot_resolve::<[i64; 12]>(null_mut()) };
        assert!(cache.is_null());
    }

    #[test]
    fn first_resolve_allocates_a_zeroed_cache_and_publishes_it() {
        let mut slot: *mut [i64; 12] = null_mut();
        let before = pic_slots_resolved();
        let cache = unsafe { pic_slot_resolve(&mut slot) };
        assert!(!cache.is_null(), "a fresh slot must resolve to a cache");
        assert_eq!(slot, cache, "the cache must be published into the slot");
        assert_eq!(
            cache as usize % PIC_CACHE_ALIGN,
            0,
            "caches are 32-byte aligned"
        );
        assert!(
            unsafe { (*cache).iter().all(|w| *w == 0) },
            "a fresh cache is all zeros"
        );
        assert!(
            pic_slots_resolved() > before,
            "the census counter must observe the allocation"
        );
    }

    #[test]
    fn resolving_a_populated_slot_returns_the_same_cache_without_allocating() {
        let mut slot: *mut [i64; 12] = null_mut();
        let first = unsafe { pic_slot_resolve(&mut slot) };
        unsafe { (*first)[0] = 0x4000_0000_0000_0001 };
        let resolved_before = pic_slots_resolved();
        let again = unsafe { pic_slot_resolve(&mut slot) };
        assert_eq!(first, again);
        assert_eq!(
            unsafe { (*again)[0] },
            0x4000_0000_0000_0001,
            "primed words survive"
        );
        assert_eq!(
            pic_slots_resolved(),
            resolved_before,
            "a second resolve of the same slot allocates nothing"
        );
    }

    #[test]
    fn peek_never_allocates() {
        let mut slot: *mut [i64; 12] = null_mut();
        let before = pic_slots_resolved();
        assert!(unsafe { pic_slot_peek(&mut slot) }.is_null());
        assert!(slot.is_null(), "peek must not publish anything");
        assert_eq!(pic_slots_resolved(), before);
        let cache = unsafe { pic_slot_resolve(&mut slot) };
        assert_eq!(unsafe { pic_slot_peek(&mut slot) }, cache);
        assert!(unsafe { pic_slot_peek::<[i64; 12]>(null_mut()) }.is_null());
    }

    #[test]
    fn a_stack_cache_pre_seeded_into_a_slot_is_honoured() {
        let mut cache = [7i64; 8];
        let mut slot: *mut [i64; 8] = &mut cache;
        let resolved = unsafe { pic_slot_resolve(&mut slot) };
        assert_eq!(resolved, &mut cache as *mut [i64; 8]);
    }

    #[test]
    fn consecutive_caches_are_packed_and_distinct() {
        let mut a: *mut [i64; 12] = null_mut();
        let mut b: *mut [i64; 12] = null_mut();
        let ca = unsafe { pic_slot_resolve(&mut a) };
        let cb = unsafe { pic_slot_resolve(&mut b) };
        assert_ne!(ca, cb);
        // Other tests allocate concurrently, so only the lower bound is exact:
        // two caches can never overlap.
        let (lo, hi) = if (ca as usize) < (cb as usize) {
            (ca, cb)
        } else {
            (cb, ca)
        };
        assert!(hi as usize - lo as usize >= 96, "caches must not overlap");
    }

    #[test]
    fn eight_word_caches_fit_the_same_arena() {
        let mut slot: *mut [i64; 8] = null_mut();
        let cache = unsafe { pic_slot_resolve(&mut slot) };
        assert!(!cache.is_null());
        assert_eq!(cache as usize % PIC_CACHE_ALIGN, 0);
        unsafe { (*cache)[7] = 1 };
        assert_eq!(unsafe { (*slot)[7] }, 1);
    }

    #[test]
    fn concurrent_first_misses_agree_on_one_cache() {
        use std::sync::atomic::AtomicUsize as Shared;
        static SLOT: Shared = Shared::new(0);
        let slot_addr = &SLOT as *const Shared as usize;
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(move || unsafe {
                    pic_slot_resolve(slot_addr as *mut *mut [i64; 12]) as usize
                })
            })
            .collect();
        let results: Vec<usize> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(
            results.iter().all(|r| *r == results[0]),
            "every thread must see one cache"
        );
        assert_eq!(SLOT.load(Ordering::Relaxed), results[0]);
    }
}
