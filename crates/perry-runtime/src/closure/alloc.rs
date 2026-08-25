//! ClosureHeader, allocation, singleton caches, and capture get/set FFI.

use super::*;
use std::cell::RefCell;

crate::perry_thread_local! {
    /// Singleton cache keyed by `func_ptr` for non-capturing closures.
    /// See `js_closure_alloc_singleton` and `scan_singleton_closure_roots_mut`.
    /// Pointer-keyed; uses `PtrHasher` (Fibonacci-multiplicative) to
    /// skip SipHash's per-byte cost — the function-pointer keys never
    /// come from external input and are already ~uniformly distributed.
    static SINGLETON_CLOSURES: RefCell<crate::fast_hash::PtrHashMap<usize, *mut ClosureHeader>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Per-`func_ptr` small-LRU cache. Each value holds up to
    /// `MAX_CAPTURED_CLOSURE_SLOTS` (captures-bits, ClosureHeader)
    /// pairs. Multiple slots are critical for the parallel-instance
    /// async-await pattern (e.g. `Promise.all` of N async closures
    /// each capturing its own boxed `__async_step`), where a single-
    /// slot cache evicts every cycle and effectively never hits.
    /// `PtrHasher`-keyed for the same reason as the other registries
    /// here — on `promise_all_chains` this is hit on every closure
    /// alloc (150 k/run).
    static SINGLETON_CAPTURED_CLOSURES: RefCell<crate::fast_hash::PtrHashMap<usize, CapturedClosureCache>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

#[derive(Clone)]
struct CapturedClosureEntry {
    /// Non-semantic prefilter for `captures`. Hash collisions always fall
    /// through to the exact bitwise tuple comparison below.
    fingerprint: u64,
    captures: Vec<u64>,
    closure: *mut ClosureHeader,
    last_used: u64,
}

/// Direct-mapped index into the exact entry vector. A collision only falls
/// back to the bounded scan; neither the fingerprint nor this hint is ever
/// trusted without exact capture-bit equality.
const CAPTURED_HINT_SLOTS: usize = 128;

struct CapturedClosureCache {
    entries: Vec<CapturedClosureEntry>,
    hint_fingerprints: [u64; CAPTURED_HINT_SLOTS],
    hint_indices_plus_one: [u8; CAPTURED_HINT_SLOTS],
    clock: u64,
}

impl CapturedClosureCache {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            hint_fingerprints: [0; CAPTURED_HINT_SLOTS],
            hint_indices_plus_one: [0; CAPTURED_HINT_SLOTS],
            clock: 0,
        }
    }

    #[inline]
    fn hint_slot(fingerprint: u64) -> usize {
        // Fold high bits before masking: capture pointers are aligned and FNV's
        // low bits alone otherwise make avoidable direct-map collisions.
        (fingerprint ^ (fingerprint >> 32)) as usize & (CAPTURED_HINT_SLOTS - 1)
    }

    #[inline]
    fn touch(&mut self, index: usize) -> *mut ClosureHeader {
        self.clock = self.clock.wrapping_add(1);
        self.entries[index].last_used = self.clock;
        self.entries[index].closure
    }

    fn lookup(&mut self, fingerprint: u64, captures: &[u64]) -> Option<*mut ClosureHeader> {
        let hint_slot = Self::hint_slot(fingerprint);
        let hinted = self.hint_indices_plus_one[hint_slot];
        if hinted != 0 && self.hint_fingerprints[hint_slot] == fingerprint {
            let index = (hinted - 1) as usize;
            if self
                .entries
                .get(index)
                .is_some_and(|entry| entry.captures.as_slice() == captures)
            {
                return Some(self.touch(index));
            }
        }

        let index = self.entries.iter().position(|entry| {
            entry.fingerprint == fingerprint && entry.captures.as_slice() == captures
        })?;
        self.hint_fingerprints[hint_slot] = fingerprint;
        self.hint_indices_plus_one[hint_slot] = (index + 1) as u8;
        Some(self.touch(index))
    }

    fn insert(&mut self, fingerprint: u64, captures: Vec<u64>, closure: *mut ClosureHeader) {
        self.clock = self.clock.wrapping_add(1);
        let entry = CapturedClosureEntry {
            fingerprint,
            captures,
            closure,
            last_used: self.clock,
        };
        let index = if self.entries.len() < MAX_CAPTURED_CLOSURE_SLOTS {
            let index = self.entries.len();
            self.entries.push(entry);
            index
        } else {
            let (index, _) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .expect("full captured-closure cache must have an LRU entry");
            self.entries[index] = entry;
            index
        };
        let hint_slot = Self::hint_slot(fingerprint);
        self.hint_fingerprints[hint_slot] = fingerprint;
        self.hint_indices_plus_one[hint_slot] = (index + 1) as u8;
    }

    fn clear_hints(&mut self) {
        self.hint_indices_plus_one.fill(0);
    }
}

#[cfg(test)]
mod captured_closure_cache_tests {
    use super::*;

    fn fake_closure(id: usize) -> *mut ClosureHeader {
        // The cache treats these as opaque values. No test dereferences them.
        (0x1000 + id * std::mem::align_of::<ClosureHeader>()) as *mut ClosureHeader
    }

    #[test]
    fn direct_hint_collision_falls_back_to_exact_capture_match() {
        let first = [0u64];
        let first_fingerprint = capture_fingerprint(&first);
        let first_slot = CapturedClosureCache::hint_slot(first_fingerprint);
        let second_word = (1..10_000u64)
            .find(|&word| {
                let fingerprint = capture_fingerprint(&[word]);
                fingerprint != first_fingerprint
                    && CapturedClosureCache::hint_slot(fingerprint) == first_slot
            })
            .expect("the bounded search must find a direct-hint collision");
        let second = [second_word];
        let second_fingerprint = capture_fingerprint(&second);

        let mut cache = CapturedClosureCache::new();
        cache.insert(first_fingerprint, first.to_vec(), fake_closure(1));
        cache.insert(second_fingerprint, second.to_vec(), fake_closure(2));

        // Inserting `second` displaced `first` from their shared hint slot.
        // Both lookups must still find their exact tuple via fallback, and
        // each fallback must repair the hint for the next lookup.
        assert_eq!(
            cache.lookup(first_fingerprint, &first),
            Some(fake_closure(1))
        );
        assert_eq!(
            cache.lookup(first_fingerprint, &first),
            Some(fake_closure(1))
        );
        assert_eq!(
            cache.lookup(second_fingerprint, &second),
            Some(fake_closure(2))
        );
        assert_eq!(cache.lookup(first_fingerprint, &second), None);
    }

    #[test]
    fn full_cache_evicts_least_recently_used_entry() {
        let mut cache = CapturedClosureCache::new();
        for word in 0..MAX_CAPTURED_CLOSURE_SLOTS as u64 {
            let captures = vec![word];
            cache.insert(
                capture_fingerprint(&captures),
                captures,
                fake_closure(word as usize + 1),
            );
        }

        let retained = [0u64];
        assert_eq!(
            cache.lookup(capture_fingerprint(&retained), &retained),
            Some(fake_closure(1))
        );

        let replacement = [MAX_CAPTURED_CLOSURE_SLOTS as u64];
        cache.insert(
            capture_fingerprint(&replacement),
            replacement.to_vec(),
            fake_closure(MAX_CAPTURED_CLOSURE_SLOTS + 1),
        );

        let evicted = [1u64];
        assert_eq!(cache.lookup(capture_fingerprint(&evicted), &evicted), None);
        assert_eq!(
            cache.lookup(capture_fingerprint(&retained), &retained),
            Some(fake_closure(1))
        );
        assert_eq!(
            cache.lookup(capture_fingerprint(&replacement), &replacement),
            Some(fake_closure(MAX_CAPTURED_CLOSURE_SLOTS + 1))
        );
    }
}

#[inline]
fn capture_fingerprint(captures: &[u64]) -> u64 {
    // FNV-1a over fixed-width capture words. The cache never trusts this as an
    // identity: it only avoids calling slice equality (and its outlined
    // `memcmp`) for entries that cannot possibly match.
    let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ captures.len() as u64;
    for &word in captures {
        hash ^= word;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Header for heap-allocated closures
#[repr(C)]
pub struct ClosureHeader {
    /// Function pointer (the actual compiled function)
    pub func_ptr: *const u8,
    /// Number of captured values
    pub capture_count: u32,
    /// Type tag: set to CLOSURE_MAGIC to identify closures at runtime
    pub type_tag: u32,
}

/// Byte offset of `type_tag` (the `CLOSURE_MAGIC` slot) within `ClosureHeader`.
///
/// On 64-bit targets this is 12 (`func_ptr` 8 bytes + `capture_count` 4 bytes);
/// on arm64_32 / wasm32 (32-bit pointers) `func_ptr` is 4 bytes, so it is 8.
/// Every site that probes a heap pointer for `CLOSURE_MAGIC` MUST read at this
/// offset, never a hardcoded `12`: that literal was correct only for 64-bit and
/// was the arm64_32 watchOS startup-crash root cause. On a 32-bit watch every
/// real closure failed the magic probe (the read landed 4 bytes past
/// `type_tag`), so a getter/function value was judged non-callable and the
/// resulting `TypeError` value-coercion dereferenced the closure as an
/// `ObjectHeader` → `EXC_BAD_ACCESS` before the first frame rendered.
/// `offset_of!` tracks the real per-target layout, so this is a no-op on 64-bit.
pub const CLOSURE_TYPE_TAG_OFFSET: usize = std::mem::offset_of!(ClosureHeader, type_tag);

#[inline]
pub fn closure_payload_size(actual_count: usize) -> usize {
    std::mem::size_of::<ClosureHeader>() + actual_count * std::mem::size_of::<u64>()
}

#[inline]
pub fn closure_alloc_storage(actual_count: usize) -> *mut u8 {
    let payload = closure_payload_size(actual_count);
    if crate::gc::GC_HEADER_SIZE + payload <= crate::gc::LARGE_OBJECT_THRESHOLD_BYTES {
        crate::arena::arena_alloc_gc(
            payload,
            std::mem::align_of::<ClosureHeader>(),
            crate::gc::GC_TYPE_CLOSURE,
        )
    } else {
        crate::gc::gc_malloc(payload, crate::gc::GC_TYPE_CLOSURE)
    }
}

#[inline]
pub unsafe fn closure_capture_slots_mut(closure: *mut ClosureHeader) -> *mut u64 {
    (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut u64
}

#[inline]
unsafe fn closure_capture_slots(closure: *const ClosureHeader) -> *const u64 {
    (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const u64
}

#[inline]
pub unsafe fn note_closure_capture_slot(
    closure: *mut ClosureHeader,
    index: usize,
    value_bits: u64,
) {
    // Standard generational-GC discipline: callers store `value_bits` into the
    // slot *before* calling here; we then record the layout bit and fire the
    // post-store write barrier. The captured value remains rooted on the
    // caller's Rust stack between the store and this call, so a minor GC
    // triggered in that window cannot drop it.
    let slot = closure_capture_slots_mut(closure).add(index);
    crate::gc::layout_note_slot(closure as usize, index, value_bits);
    crate::gc::runtime_write_barrier_gc_slot(closure as usize, slot as usize, value_bits);
}

#[inline]
pub unsafe fn rebuild_closure_layout_and_barriers(closure: *mut ClosureHeader, slot_count: usize) {
    let slots = closure_capture_slots_mut(closure);
    crate::gc::layout_rebuild_from_slots(closure as *mut u8, slots as *const u64, slot_count);
    for i in 0..slot_count {
        let slot = slots.add(i);
        crate::gc::runtime_write_barrier_slot(closure as usize, slot as usize, *slot);
    }
}

pub(crate) unsafe fn gc_capture_slot_range(
    closure: *mut ClosureHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if closure.is_null() {
        return None;
    }
    let capture_count = real_capture_count((*closure).capture_count) as usize;
    if capture_count > 1_000_000 {
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        closure_capture_slots_mut(closure),
        capture_count,
    ))
}

/// Allocate a closure with space for captured values.
/// The high bit of `capture_count` may contain CAPTURES_THIS_FLAG to indicate
/// that slot 0 is reserved for `this`. The flag is preserved in the header
/// for later use by `js_closure_unbind_this`, but the actual allocation size
/// uses only the lower 31 bits.
/// Returns pointer to ClosureHeader
#[no_mangle]
pub extern "C" fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut ClosureHeader {
    crate::promise::bump(&CLOSURE_ALLOC_COUNT);
    let actual_count = real_capture_count(capture_count) as usize;

    let raw = closure_alloc_storage(actual_count);
    let ptr = raw as *mut ClosureHeader;

    unsafe {
        (*ptr).func_ptr = func_ptr;
        (*ptr).capture_count = capture_count; // Preserve flag in high bit
        (*ptr).type_tag = CLOSURE_MAGIC;
        // #7154: a fresh closure's capture slots are raw recycled arena bytes.
        // They are invisible to the collector while the layout says
        // POINTER_FREE, but any code path (conservative scan, diagnostic
        // from-space scan, a later layout rebuild over the whole slot range)
        // that reads them decodes garbage as a reference. Initialize them to a
        // non-pointer sentinel, mirroring what `js_object_alloc` does for
        // object fields and what #7138 did for unused array capacity.
        let slots = closure_capture_slots_mut(ptr);
        for i in 0..actual_count {
            // GC_STORE_AUDIT(INIT): fresh closure capture slot, pointer-free sentinel.
            std::ptr::write(slots.add(i), crate::value::TAG_UNDEFINED);
        }
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

pub static CLOSURE_ALLOC_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static CLOSURE_CAP_SINGLETON_HIT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub static CLOSURE_CAP_SINGLETON_MISS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Singleton-cached closure allocation for non-capturing closures and FuncRef
/// wrappers. The same `func_ptr` always yields the SAME ClosureHeader, so a
/// hot loop like `arr.filter(x => x.kind === 'foo')` doesn't allocate (and
/// trigger GC against) a fresh closure on every iteration.
///
/// Per-call cost: one thread-local hashmap lookup + one branch + one load.
/// Avoids even the nursery allocation for hot no-capture cases — a single
/// hot non-capturing closure inside a tight for-loop used to be a visible
/// allocation source in sync-hotpath / perf-comprehensive.
///
/// Safety: the cached closure has zero captures, so it has no per-call
/// state — sharing it across all call sites is observationally identical
/// to allocating fresh. The closure is GC-rooted by the singleton table's
/// mutable scanner so it stays live across collections.
#[no_mangle]
pub extern "C" fn js_closure_alloc_singleton(func_ptr: *const u8) -> *mut ClosureHeader {
    // Fast path: already cached. Drop the borrow before any potential
    // alloc so allocation/GC can re-enter SINGLETON_CLOSURES if needed.
    if let Some(cached) = SINGLETON_CLOSURES.with(|s| s.borrow().get(&(func_ptr as usize)).copied())
    {
        return cached;
    }
    let allocated = js_closure_alloc(func_ptr, 0);
    SINGLETON_CLOSURES.with(|s| {
        s.borrow_mut().insert(func_ptr as usize, allocated);
    });
    crate::gc::runtime_write_barrier_root_heap_word(allocated as u64);
    allocated
}

/// Mutable GC scanner for singleton closure caches.
///
/// No-capture cache values are raw closure pointers. Captured cache entries
/// additionally keep a bit-exact capture tuple as the cache key; each key word
/// can be a NaN-boxed JSValue or a raw heap pointer, matching closure capture
/// storage. The mutable visitor lets copied-minor rewrite both the closure's
/// heap capture slots and the cache key words after moving young captures.
pub fn scan_singleton_closure_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SINGLETON_CLOSURES.with(|s| {
        let mut closures = s.borrow_mut();
        for closure in closures.values_mut() {
            visitor.visit_raw_mut_ptr_slot(closure);
        }
    });
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        let mut captured = s.borrow_mut();
        for cache in captured.values_mut() {
            for entry in cache.entries.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(&mut entry.closure);
                for word in entry.captures.iter_mut() {
                    visitor.visit_heap_word_u64_slot(word);
                }
                // A copying collection may have rewritten pointer-bearing
                // capture words. Keep the non-semantic prefilter synchronized
                // with the exact tuple that remains authoritative.
                entry.fingerprint = capture_fingerprint(&entry.captures);
            }
            // Fingerprints and exact tuples were just rewritten in place. A
            // hint is disposable acceleration state; clearing avoids an
            // address-derived pre-GC fingerprint/index ever being consulted.
            cache.clear_hints();
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_singleton_closure_caches() {
    SINGLETON_CLOSURES.with(|s| s.borrow_mut().clear());
    SINGLETON_CAPTURED_CLOSURES.with(|s| s.borrow_mut().clear());
    CAPTURED_MISS_STREAK.with(|s| s.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn test_seed_singleton_closure_cache(func_ptr: *const u8, closure: *mut ClosureHeader) {
    SINGLETON_CLOSURES.with(|s| {
        s.borrow_mut().insert(func_ptr as usize, closure);
    });
}

#[cfg(test)]
pub(crate) fn test_seed_captured_singleton_closure_cache(
    func_ptr: *const u8,
    capture_key: Vec<u64>,
    closure: *mut ClosureHeader,
) {
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        s.borrow_mut()
            .entry(func_ptr as usize)
            .or_insert_with(CapturedClosureCache::new)
            .insert(capture_fingerprint(&capture_key), capture_key, closure);
    });
}

#[cfg(test)]
pub(crate) fn test_singleton_closure_cache_entry(
    func_ptr: *const u8,
) -> Option<*mut ClosureHeader> {
    SINGLETON_CLOSURES.with(|s| s.borrow().get(&(func_ptr as usize)).copied())
}

#[cfg(test)]
pub(crate) fn test_captured_singleton_closure_cache_entries(
    func_ptr: *const u8,
) -> Vec<(Vec<u64>, *mut ClosureHeader)> {
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        s.borrow()
            .get(&(func_ptr as usize))
            .map(|cache| {
                cache
                    .entries
                    .iter()
                    .map(|entry| (entry.captures.clone(), entry.closure))
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Maximum number of (captures-tuple, ClosureHeader) entries cached
/// per-`func_ptr` in `SINGLETON_CAPTURED_CLOSURES`. Sized to absorb the
/// parallel-instance async-await pattern (e.g. `Promise.all` of N
/// concurrent unitOfWork calls each capturing their own boxed
/// `__async_step`) without filling the cache when N is large. The
/// LRU eviction inside the slot list keeps the most-recently-seen
/// entries hot. Empirical: capping at 64 keeps memory bounded but
/// covers the per-batch fan-out shape (50 promises) found in
/// `benchmarks/app-patterns/kernels/promise_all_chains.ts`.
const MAX_CAPTURED_CLOSURE_SLOTS: usize = 64;

/// Per-`func_ptr` cache miss-streak counter for the adaptive bypass.
/// Closures whose captures change every call (per-call boxes for
/// `__step` / `__gen_state`, etc.) miss 100% of the time on the
/// captures-tuple cache; after `CAPTURED_MISS_STREAK_DISABLE` consecutive
/// misses we mark the `func_ptr` as "cache-disabled" and route it to a
/// direct `js_closure_alloc + memcpy` with no HashMap touch, no Vec scan,
/// no Vec::to_vec capture-tuple allocation. A future hit (e.g. if the
/// workload changes shape and captures stabilise) resets the counter.
const CAPTURED_MISS_STREAK_DISABLE: u32 = 256;
const CAPTURED_DISABLED_SENTINEL: u32 = u32::MAX;

crate::perry_thread_local! {
    static CAPTURED_MISS_STREAK: RefCell<crate::fast_hash::PtrHashMap<usize, u32>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Per-`func_ptr` single-slot cache for closures with captures. When
/// the same closure literal is created again with the SAME capture
/// bits, we return the cached closure; otherwise we allocate a fresh
/// one and replace the slot.
///
/// `captures_ptr` points at `capture_count` consecutive 8-byte values
/// matching the layout `js_closure_set_capture_f64` writes.
///
/// One entry per closure literal (bounded by program size). Closures
/// whose captures vary per call (e.g. `getOrCompute(map, key, () =>
/// ...)` capturing a fresh array each call) miss every time but only
/// occupy one slot, so they don't crowd out steady-state captures.
#[no_mangle]
pub extern "C" fn js_closure_alloc_with_captures_singleton(
    func_ptr: *const u8,
    capture_count: u32,
    captures_ptr: *const u64,
) -> *mut ClosureHeader {
    let n = real_capture_count(capture_count) as usize;
    let captures_slice: &[u64] = if n == 0 || captures_ptr.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(captures_ptr, n) }
    };
    let fingerprint = capture_fingerprint(captures_slice);

    // Adaptive bypass: if this func_ptr has missed the cache N times in
    // a row, skip the cache entirely. Async-step closures (`__step` /
    // `next` / `throw` / `__then_v` / `__then_e`) all capture a fresh
    // box pointer per invocation so they miss 100% of the time; the
    // bypass turns cache-lookup overhead into a direct allocation + memcpy.
    let streak =
        CAPTURED_MISS_STREAK.with(|m| m.borrow().get(&(func_ptr as usize)).copied().unwrap_or(0));
    if streak == CAPTURED_DISABLED_SENTINEL {
        crate::promise::bump(&CLOSURE_CAP_SINGLETON_MISS);
        let capture_scope = crate::gc::RuntimeHandleScope::new();
        let capture_handles: Vec<_> = captures_slice
            .iter()
            .map(|bits| capture_scope.root_heap_word_u64(*bits))
            .collect();
        let allocated = js_closure_alloc(func_ptr, capture_count);
        if n > 0 && !captures_ptr.is_null() {
            let rewritten_captures: Vec<u64> = capture_handles
                .iter()
                .map(|handle| handle.get_heap_word_u64())
                .collect();
            unsafe {
                let dest = closure_capture_slots_mut(allocated);
                // GC_STORE_AUDIT(BARRIERED): copied captures are followed by closure layout/barrier rebuild.
                std::ptr::copy_nonoverlapping(rewritten_captures.as_ptr(), dest, n);
                rebuild_closure_layout_and_barriers(allocated, n);
            }
        }
        return allocated;
    }

    // Fast path: use the tuple fingerprint's direct-mapped hint, then exact
    // bit-equality of every capture slot. Hint collisions fall back to the
    // bounded entry scan and repair the hint. Entries carry a monotonic
    // last-use timestamp, so lookups no longer memmove the Vec yet full-cache
    // eviction preserves the same least-recently-used policy.
    if let Some(cached) = SINGLETON_CAPTURED_CLOSURES.with(|s| {
        let mut s = s.borrow_mut();
        s.get_mut(&(func_ptr as usize))
            .and_then(|cache| cache.lookup(fingerprint, captures_slice))
    }) {
        crate::promise::bump(&CLOSURE_CAP_SINGLETON_HIT);
        // Cache hit — reset the streak so a workload that briefly
        // thrashed then settled into stable captures gets caching back.
        CAPTURED_MISS_STREAK.with(|m| {
            m.borrow_mut().insert(func_ptr as usize, 0);
        });
        return cached;
    }
    crate::promise::bump(&CLOSURE_CAP_SINGLETON_MISS);

    // Slow path: allocate, populate captures, insert into cache as
    // the most-recent entry. If the slot list is full, drop the
    // least-recent (back of the Vec).
    let capture_scope = crate::gc::RuntimeHandleScope::new();
    let capture_handles: Vec<_> = captures_slice
        .iter()
        .map(|bits| capture_scope.root_heap_word_u64(*bits))
        .collect();
    let allocated = js_closure_alloc(func_ptr, capture_count);
    let rewritten_captures: Vec<u64> = capture_handles
        .iter()
        .map(|handle| handle.get_heap_word_u64())
        .collect();
    if n > 0 && !captures_ptr.is_null() {
        unsafe {
            let dest = closure_capture_slots_mut(allocated);
            // GC_STORE_AUDIT(BARRIERED): cached closure captures are followed by layout/barrier rebuild.
            std::ptr::copy_nonoverlapping(rewritten_captures.as_ptr(), dest, n);
            rebuild_closure_layout_and_barriers(allocated, n);
        }
    }
    crate::gc::runtime_write_barrier_root_heap_word(allocated as u64);
    for &bits in &rewritten_captures {
        crate::gc::runtime_write_barrier_root_heap_word(bits);
    }
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        let mut s = s.borrow_mut();
        s.entry(func_ptr as usize)
            .or_insert_with(CapturedClosureCache::new)
            .insert(
                capture_fingerprint(&rewritten_captures),
                rewritten_captures,
                allocated,
            );
    });
    // Bump the miss-streak counter; flip to disabled sentinel when we
    // hit the threshold.
    CAPTURED_MISS_STREAK.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.entry(func_ptr as usize).or_insert(0);
        if *entry < CAPTURED_DISABLED_SENTINEL - 1 {
            *entry += 1;
            if *entry >= CAPTURED_MISS_STREAK_DISABLE {
                *entry = CAPTURED_DISABLED_SENTINEL;
            }
        }
    });
    allocated
}

/// Get the function pointer from a closure
#[no_mangle]
pub extern "C" fn js_closure_get_func(closure: *const ClosureHeader) -> *const u8 {
    unsafe { (*closure).func_ptr }
}

/// Get a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64 {
    f64::from_bits(js_closure_get_capture_bits(closure, index))
}

/// Set a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64) {
    js_closure_set_capture_bits(closure, index, value.to_bits());
}

/// Get a captured value's raw JSValueBits by index.
#[no_mangle]
pub extern "C" fn js_closure_get_capture_bits(closure: *const ClosureHeader, index: u32) -> u64 {
    if closure.is_null() {
        return 0;
    }
    unsafe {
        if index as usize >= real_capture_count((*closure).capture_count) as usize {
            return 0;
        }
        *closure_capture_slots(closure).add(index as usize)
    }
}

/// Set a captured value's raw JSValueBits by index.
#[no_mangle]
pub extern "C" fn js_closure_set_capture_bits(
    closure: *mut ClosureHeader,
    index: u32,
    value_bits: u64,
) {
    if closure.is_null() {
        return;
    }
    unsafe {
        let captures_ptr = closure_capture_slots_mut(closure);
        // GC_STORE_AUDIT(BARRIERED): closure bits capture write is immediately recorded via note_closure_capture_slot.
        *captures_ptr.add(index as usize) = value_bits;
        note_closure_capture_slot(closure, index as usize, value_bits);
    }
}

/// Set a capture slot which codegen has proven contains a raw variable-box
/// pointer. Keeping this separate from the generic setter prevents arbitrary
/// pointer-shaped JS values from becoming false box-lifetime edges.
#[no_mangle]
pub extern "C" fn js_closure_set_box_capture_ptr(
    closure: *mut ClosureHeader,
    index: u32,
    value: i64,
) {
    js_closure_set_capture_bits(closure, index, value as u64);
    let cell = crate::r#box::registered_box_capture_addr(value as usize);
    super::box_captures::set_closure_box_capture(closure, index, cell);
}

/// Get a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_ptr(closure: *const ClosureHeader, index: u32) -> i64 {
    js_closure_get_capture_bits(closure, index) as i64
}

/// Set a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_ptr(closure: *mut ClosureHeader, index: u32, value: i64) {
    js_closure_set_capture_bits(closure, index, value as u64);
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CLOSURE_GET_CAPTURE_BITS: extern "C" fn(*const ClosureHeader, u32) -> u64 =
    js_closure_get_capture_bits;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CLOSURE_SET_CAPTURE_BITS: extern "C" fn(*mut ClosureHeader, u32, u64) =
    js_closure_set_capture_bits;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_CLOSURE_SET_BOX_CAPTURE_PTR: extern "C" fn(*mut ClosureHeader, u32, i64) =
    js_closure_set_box_capture_ptr;
