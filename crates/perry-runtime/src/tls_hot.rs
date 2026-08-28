//! One thread-local resolution for the whole allocation hot path (#7469).
//!
//! # Why this exists
//!
//! On Darwin every `thread_local!` access is an out-of-line call to
//! `_tlv_get_addr` in `libdyld`. Unlike ELF's `local-exec` / `initial-exec`
//! models it is a real call — not inlined, not cached across accesses, and it
//! clobbers caller-saved registers at the site. LLVM *can* CSE repeated
//! accesses to the **same** thread-local within a function, but two different
//! thread-locals are two different descriptors, so N distinct thread-locals on
//! one code path cost N calls no matter how well the path inlines.
//!
//! The runtime declares 237 `thread_local!` blocks, and a single
//! `{v, w}` object literal touches roughly a dozen of them: the arena and its
//! inline bump state, the free-list flag, the birth-flag cell, the layout side
//! tables, the page-generation cache the write barrier classifies against, and
//! the temp-root stack. Measured on `gc-handoff/bench/churn.ts` at
//! `351742d30`, `_tlv_get_addr` was **34.2% of all self time** — more than the
//! allocation work it was gating, and invisible to `PERRY_GC_TRACE` because it
//! is mutator time, not pause time.
//!
//! # What this does
//!
//! [`HotTls`] caches the **addresses** of those thread-locals in one
//! `const`-initialised thread-local. The storage does not move: every field is
//! the address of the existing `thread_local!` in its owning module, so
//! initialisation order, lazy init, and destructor registration are all
//! unchanged. A hot path that used to pay N `_tlv_get_addr` calls now pays one
//! (for `HOT` itself, which LLVM then CSEs across the whole inlined region)
//! plus N loads from one cache line.
//!
//! # Do not add a field — declare with [`crate::perry_thread_local`]
//!
//! The sixteen named fields below are a **closed set**. They are the
//! allocation path, they have fixed offsets, and they are kept because a fixed
//! offset is one load cheaper than a claimed slot on the hottest path in the
//! runtime. Nothing else belongs there.
//!
//! Adding one used to take four manual steps — slot, `…_hot_addr()` provider,
//! a line in [`fill`], and a line in
//! `tests::cached_addresses_match_thread_locals` — and step four was
//! load-bearing, because the slots are untyped (`*mut u8`) so a mis-wired
//! `fill` would hand out a correctly-typed reference to the *wrong* object.
//!
//! That contract cannot scale to ~520 declarations, and it gets the default
//! backwards: forgetting it produces a **working slow path**, not a build
//! error. Which is why this cost was fixed three times and came back three
//! times — measured 0% of `churn_alloc` after #7565 (`churn` is covered by
//! construction), 8-9% later, 11% on `interp`/`retain`, and 20.5% of
//! `asyncpipe`, whose Map/Set registries, buffer brands and descriptor state
//! were on nobody's list.
//!
//! [`crate::perry_thread_local`] is the default now: same syntax as
//! `thread_local!`, same `with`/`try_with` at every call site, and the address
//! lands in a generic slot of this same cache with **nothing to wire**. The
//! declaration generates its own storage, its own resolver and its own typed
//! key, so the cross-cast hazard above cannot be expressed — and it installs a
//! teardown guard exactly when the value has a destructor, which the named
//! fields do not have at all. `scripts/check_thread_locals.py` makes a new raw
//! `thread_local!` a build error unless it is recorded as deliberately cold,
//! and `scripts/tls_budget_gate.sh` measures the outcome on two programs whose
//! paths are deliberately *not* among these sixteen.
//!
//! # Lifetime
//!
//! The accessors hand out `&'static` references. That is sound for the *cache*
//! (const-init, no `Drop`, so it is never destroyed) and carries exactly the
//! same thread-teardown exposure the runtime already has for `ARENA` and
//! `INLINE_STATE`, whose raw pointers are handed to generated code by
//! `js_inline_arena_state`. It is not an invitation to send one across
//! threads, and the pointee types (`Cell`, `RefCell`, `UnsafeCell`) are all
//! `!Sync`, so the compiler refuses that on its own.
//!
//! # Reaching the cache without a TLS resolution (#7469 structural half)
//!
//! Caching the addresses removed the *per-field* resolutions but left one:
//! `HOT` is itself a `thread_local!`, so every runtime function that reads any
//! hot field still paid one `_tlv_get_addr` call. Symbolicated on the pinned
//! quiet host at `9938cbc1a`, that residue was **27.0% of `churn_alloc`**, and
//! its call-graph attribution was not diffuse: **seven functions carried 98%
//! of it, and every one of the seven resolves `HOT`** —
//! `write_barrier_decoded_parent` (19.3%), `layout_forget_object` (18.9%),
//! `js_object_alloc_class_inline_keys` (18.5%), `arena_alloc` (14.9%),
//! `js_write_barrier_slot` (9.2%), `barrier_child_prologue` (8.8%) and
//! `typed_shape_layout_entry` (8.4%). Two of them resolve *nothing else*.
//!
//! So the structural fix is to make reaching `HOT` free rather than to thread a
//! context pointer through 2994 `.with()` sites. On Apple aarch64 the pthread
//! thread-specific-data array is directly addressable from `TPIDRRO_EL0` —
//! that is how `pthread_getspecific` itself is implemented, and what mimalloc
//! (already linked into this runtime) does on this platform. Publishing the
//! cache's address into one `pthread_key_create` slot turns the resolution
//! from an out-of-line call that clobbers caller-saved registers into `mrs` +
//! two loads that LLVM can CSE across a whole function.
//!
//! [`darwin_tsd`] carries the self-check that makes this safe to ship: the
//! publishing thread reads the slot back through the direct path and compares
//! it against what `pthread_setspecific` was handed. A mismatch — the shape a
//! future OS change would take — disables the direct path process-wide and
//! every thread falls back to `_tlv_get_addr`, permanently and silently
//! correctly. It cannot degrade into reading a wrong address.

use std::cell::{Cell, UnsafeCell};

/// How many generic [`HotKey`] slots one thread's cache can hold.
///
/// One `*mut u8` each, in `__thread_bss`, so the cost is address space rather
/// than image size. `perry-runtime` declares ~520 `thread_local!`s in total, so
/// this leaves headroom for every one of them plus `perry-stdlib`'s.
///
/// Overflow is *correct* — a declaration that cannot get a slot simply falls
/// back to the plain `thread_local!` path forever — but it is **silent**, which
/// is precisely the failure mode this file exists to abolish. So two things
/// watch it: `scripts/check_thread_locals.py` fails the build when the
/// declaration count approaches this ceiling, and `claimed_slots` lets the
/// runtime budget gate reject a run that reached it.
pub const HOT_SLOT_CAPACITY: usize = 768;

/// A [`SlotId`] that has never been claimed.
const SLOT_UNASSIGNED: u32 = u32::MAX;
/// A [`SlotId`] that asked for a slot after the last one was handed out. Both
/// sentinels are `>= HOT_SLOT_CAPACITY`, so the one bound check on the hot path
/// rejects them together.
const SLOT_OVERFLOW: u32 = u32::MAX - 1;

/// Cached addresses of the per-thread state on the allocation hot path.
///
/// Slots are untyped so each owning module keeps its storage type private;
/// the typed accessor lives next to the `thread_local!` it casts back to.
#[repr(C)]
pub(crate) struct HotTls {
    // arena/block.rs
    pub(crate) arena: *mut u8,
    pub(crate) inline_state: *mut u8,
    // arena/page_meta.rs
    pub(crate) page_generation_cache: *mut u8,
    pub(crate) page_generations: *mut u8,
    // gc/malloc.rs
    pub(crate) arena_free_list: *mut u8,
    pub(crate) arena_free_list_nonempty: *mut u8,
    // gc/barrier.rs
    pub(crate) birth_extra_flags: *mut u8,
    pub(crate) incremental_mark_valid_ptrs: *mut u8,
    pub(crate) incremental_mark_minor_only: *mut u8,
    // gc/layout.rs
    pub(crate) layout_slot_masks: *mut u8,
    pub(crate) typed_layouts: *mut u8,
    pub(crate) shape_layouts: *mut u8,
    pub(crate) per_object_layouts_nonempty: *mut u8,
    // gc/shape_install.rs
    pub(crate) shape_install_memo: *mut u8,
    // object/spill.rs
    pub(crate) learned_inline_fields: *mut u8,
    // gc/roots/temp_roots.rs
    pub(crate) temp_roots: *mut u8,
    // ------------------------------------------------------------------
    // Inline hot VALUES. A named pointer field and a generic slot both cost
    // TSD base → `HotTls` → slot pointer → value; a value that lives here is
    // one dependent load shorter (TSD base → `HotTls` → value), and on the
    // hottest probes — the write barrier's dirty-page cache on every
    // remembered store, the prototype rows on every indexed array write, the
    // box-pointer caches on every boxed-local read — that load was the
    // measurable part. Only small `Copy` values with a `const` initial state
    // belong here; anything needing `Drop` stays a slot.
    // ------------------------------------------------------------------
    /// `gc::dirty_page_cache` — the one-entry dirty-page cache
    /// (`usize::MAX` = nothing cached).
    pub(crate) last_dirty_old_page: Cell<usize>,
    /// `array::prototype_addr` — this thread's memoized intrinsic prototype
    /// addresses, `usize::MAX` = not yet computed. Rewritten by the
    /// collector's root scan like the slot it replaced.
    pub(crate) prototype_addrs: [Cell<usize>; INLINE_PROTOTYPE_ADDR_ROWS],
    /// `box` — direct-mapped positive caches over the three box registries.
    pub(crate) box_ptr_cache: [Cell<usize>; INLINE_BOX_PTR_CACHE_SLOTS],
    pub(crate) i32_box_ptr_cache: [Cell<usize>; INLINE_BOX_PTR_CACHE_SLOTS],
    pub(crate) bool_box_ptr_cache: [Cell<usize>; INLINE_BOX_PTR_CACHE_SLOTS],
    /// Generic slots, one per [`crate::perry_thread_local`] declaration that
    /// this thread has resolved at least once. Last, so the named fields above
    /// keep their small fixed offsets.
    slots: [Cell<*mut u8>; HOT_SLOT_CAPACITY],
}

/// Rows of [`HotTls::prototype_addrs`]; `array::prototype_addr` sizes its
/// builtin-name table from this.
pub(crate) const INLINE_PROTOTYPE_ADDR_ROWS: usize = 2;
/// Slots of each [`HotTls`] box-pointer cache; `box` indexes with this.
pub(crate) const INLINE_BOX_PTR_CACHE_SLOTS: usize = 8;

impl HotTls {
    /// Read a claimed slot. `idx` must have passed the `< HOT_SLOT_CAPACITY`
    /// test that both sentinels fail.
    #[inline(always)]
    fn slot(&self, idx: u32) -> *mut u8 {
        debug_assert!((idx as usize) < HOT_SLOT_CAPACITY);
        // SAFETY: the caller checked the bound; this elides the panic path from
        // every hot read, which is the whole point of the check being a single
        // unsigned compare against a constant.
        unsafe { self.slots.get_unchecked(idx as usize).get() }
    }

    #[inline(always)]
    fn set_slot(&self, idx: u32, value: *mut u8) {
        debug_assert!((idx as usize) < HOT_SLOT_CAPACITY);
        // SAFETY: as `slot` above.
        unsafe { self.slots.get_unchecked(idx as usize).set(value) }
    }

    const EMPTY: Self = Self {
        arena: std::ptr::null_mut(),
        inline_state: std::ptr::null_mut(),
        page_generation_cache: std::ptr::null_mut(),
        page_generations: std::ptr::null_mut(),
        arena_free_list: std::ptr::null_mut(),
        arena_free_list_nonempty: std::ptr::null_mut(),
        birth_extra_flags: std::ptr::null_mut(),
        incremental_mark_valid_ptrs: std::ptr::null_mut(),
        incremental_mark_minor_only: std::ptr::null_mut(),
        layout_slot_masks: std::ptr::null_mut(),
        typed_layouts: std::ptr::null_mut(),
        shape_layouts: std::ptr::null_mut(),
        per_object_layouts_nonempty: std::ptr::null_mut(),
        shape_install_memo: std::ptr::null_mut(),
        learned_inline_fields: std::ptr::null_mut(),
        temp_roots: std::ptr::null_mut(),
        last_dirty_old_page: Cell::new(usize::MAX),
        prototype_addrs: [const { Cell::new(usize::MAX) }; INLINE_PROTOTYPE_ADDR_ROWS],
        box_ptr_cache: [const { Cell::new(0) }; INLINE_BOX_PTR_CACHE_SLOTS],
        i32_box_ptr_cache: [const { Cell::new(0) }; INLINE_BOX_PTR_CACHE_SLOTS],
        bool_box_ptr_cache: [const { Cell::new(0) }; INLINE_BOX_PTR_CACHE_SLOTS],
        slots: [const { Cell::new(std::ptr::null_mut()) }; HOT_SLOT_CAPACITY],
    };
}

thread_local! {
    /// `const`-initialised on purpose: a lazily-initialised `thread_local!`
    /// pays a "has this been initialised / has this been dropped" check on
    /// every `.with()` **on top of** `_tlv_get_addr`, and registers a
    /// destructor. `HotTls` is plain pointers with no `Drop`, so the const
    /// form reduces the one remaining resolution to the bare thunk call.
    static HOT: UnsafeCell<HotTls> = const { UnsafeCell::new(HotTls::EMPTY) };
}

/// Resolve every cached address for this thread. Cold: runs once per thread.
///
/// Each `…_hot_addr()` touches its own `thread_local!` exactly as any other
/// caller would, so a lazily-initialised one is initialised here instead of at
/// its first hot-path use. That is a move in when, not in what.
#[cold]
#[inline(never)]
fn fill(slots: *mut HotTls) {
    // SAFETY: `slots` is this thread's own cache; no other thread can observe
    // it and the runtime is single-threaded per arena.
    unsafe {
        (*slots).arena = crate::arena::arena_hot_addr();
        (*slots).inline_state = crate::arena::inline_state_hot_addr();
        (*slots).page_generation_cache = crate::arena::page_generation_cache_hot_addr();
        (*slots).page_generations = crate::arena::page_generations_hot_addr();
        (*slots).arena_free_list = crate::gc::arena_free_list_hot_addr();
        (*slots).arena_free_list_nonempty = crate::gc::arena_free_list_nonempty_hot_addr();
        (*slots).birth_extra_flags = crate::gc::birth_extra_flags_hot_addr();
        (*slots).incremental_mark_valid_ptrs = crate::gc::incremental_mark_valid_ptrs_hot_addr();
        (*slots).incremental_mark_minor_only = crate::gc::incremental_mark_minor_only_hot_addr();
        (*slots).layout_slot_masks = crate::gc::layout_slot_masks_hot_addr();
        (*slots).typed_layouts = crate::gc::typed_layouts_hot_addr();
        (*slots).shape_layouts = crate::gc::shape_layouts_hot_addr();
        (*slots).per_object_layouts_nonempty = crate::gc::per_object_layouts_nonempty_hot_addr();
        (*slots).shape_install_memo = crate::gc::shape_install_memo_hot_addr();
        (*slots).learned_inline_fields = crate::object::learned_inline_fields_hot_addr();
        // Last, and the field `hot()` tests: every other slot is already
        // written by the time this one is non-null, so a re-entrant call from
        // inside one of the providers above cannot observe a half-filled cache
        // as ready.
        (*slots).temp_roots = crate::gc::temp_roots_hot_addr();
    }
}

/// Resolve this thread's cache through the ordinary TLS accessor, filling it
/// on first use. One `_tlv_get_addr` on Darwin; a `local-exec` fixed offset on
/// targets whose TLS model does not need an accessor call at all.
#[inline(always)]
fn hot_via_tls() -> &'static HotTls {
    let slots = HOT.with(|cell| cell.get());
    // SAFETY: `HOT` is const-init with no `Drop`, so its storage is valid for
    // the whole life of the thread — see the module docs on lifetime.
    unsafe {
        if (*slots).temp_roots.is_null() {
            fill(slots);
        }
        &*slots
    }
}

/// Direct pthread thread-specific-data addressing, and the self-check that
/// keeps it honest. See the module docs for why this exists.
#[cfg(all(
    target_vendor = "apple",
    target_arch = "aarch64",
    target_pointer_width = "64"
))]
pub(crate) mod darwin_tsd {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// `KEY`'s "there is no direct path" value — either the key has not been
    /// created yet, or [`publish`]'s self-check rejected it.
    pub(super) const NO_KEY: usize = usize::MAX;

    pub(super) static KEY: AtomicUsize = AtomicUsize::new(NO_KEY);

    /// Latched by [`disable`] so no later thread retries a path this process
    /// has already proven wrong.
    static DISABLED: AtomicBool = AtomicBool::new(false);

    /// Read thread-specific-data slot `slot` for the calling thread.
    ///
    /// `TPIDRRO_EL0` holds this thread's TSD base with the CPU number in the
    /// low three bits. Masking those off and indexing is precisely what
    /// `_os_tsd_get_direct` — and therefore `pthread_getspecific` — does, and
    /// what mimalloc (already linked into this runtime) does on this platform.
    ///
    /// # Safety
    /// `slot` must be a key returned by `pthread_key_create`, so that the index
    /// lands inside the thread's TSD array.
    /// This thread's TSD base — the per-thread constant [`get`] indexes from,
    /// exposed so a hot reader can *identify* the calling thread with one
    /// `mrs` and no memory access at all (the write barrier's dirty-page
    /// cache mirrors its value under the writing thread's base). Same asm and
    /// the same NOT-`pure` discipline as [`get`]: the value must be re-read
    /// wherever execution can resume on another thread.
    #[inline(always)]
    pub(crate) fn base() -> usize {
        let base: usize;
        // SAFETY: reads a user-readable system register; no memory touched.
        unsafe {
            core::arch::asm!(
                "mrs {b}, tpidrro_el0",
                b = out(reg) base,
                options(nomem, nostack, preserves_flags)
            );
        }
        base & !0b111
    }

    #[inline(always)]
    pub(super) unsafe fn get(slot: usize) -> *mut u8 {
        let base: usize;
        // **NOT `pure`. This is load-bearing and was learned the hard way.**
        //
        // `options(pure, nomem)` is the obvious choice — masked of its
        // CPU-number bits the register is a per-thread constant, so `pure`
        // lets LLVM CSE the read across a whole function and hoist it out of
        // loops, and it costs one `mrs` per reader to give that up. It is also
        // **wrong**, and the first version of this change shipped it.
        //
        // `pure` promises the result depends only on the inputs, and this asm
        // has none — so LLVM is free to compute it once and reuse the value
        // anywhere in the function, *including across a point where execution
        // resumes on a different thread*. `perry-stdlib`'s async bridge is
        // exactly that shape: `hot()` is `#[inline(always)]` and LTO inlines it
        // into futures that tokio polls, so a hoisted thread pointer outlives
        // the thread it was read on. The observable was every `node:net` /
        // `node:http` server aborting with tokio's "there is no reactor
        // running" — five out of five runs, against five out of five clean on
        // `main`, and it did not reproduce in any unit test or in the whole
        // allocation-benchmark set. Deleting `pure` fixes it.
        //
        // The tempting counter-argument — that `@llvm.threadlocal.address` is
        // already `speculatable` and `memory(none)`, so a `thread_local!` read
        // had the same freedom — does not hold: on Darwin that intrinsic
        // lowers to a *call* through the TLV descriptor, which LLVM will not
        // hoist across arbitrary code. Replacing the call with inline asm is
        // what made the hoist possible, so this is a constraint introduced
        // here, not inherited.
        core::arch::asm!(
            "mrs {b}, tpidrro_el0",
            b = out(reg) base,
            options(nomem, nostack, preserves_flags)
        );
        // SAFETY: the caller guarantees `slot` is a live pthread key.
        unsafe { *((base & !0b111) as *const *mut u8).add(slot) }
    }

    /// Publish `value` as this thread's cache address, then prove the direct
    /// read agrees with what `pthread_setspecific` was handed.
    ///
    /// The check is the whole reason this is safe to ship: if a future OS ever
    /// moves the TSD array out from under `TPIDRRO_EL0`, the very first thread
    /// notices here and the process reverts to `_tlv_get_addr` for good. There
    /// is no path on which a mismatch turns into a wrong address being read on
    /// the allocation hot path.
    pub(super) fn publish(value: *mut u8) {
        if DISABLED.load(Ordering::Relaxed) {
            return;
        }
        let key = ensure_key();
        if key == NO_KEY {
            return;
        }
        // SAFETY: `key` came from `pthread_key_create` below.
        let rc = unsafe { libc::pthread_setspecific(key as libc::pthread_key_t, value.cast()) };
        // SAFETY: as above — and this is exactly the read `pthread_getspecific`
        // would perform for the same key on this thread.
        if rc != 0 || unsafe { get(key) } != value {
            disable(key);
        }
    }

    fn ensure_key() -> usize {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let mut key: libc::pthread_key_t = 0;
            // No destructor: the cache is the `HOT` thread-local's own storage,
            // owned by the TLS runtime. This key only borrows its address.
            // SAFETY: `key` is a live local for the duration of the call.
            if unsafe { libc::pthread_key_create(&mut key, None) } == 0 {
                KEY.store(key as usize, Ordering::Release);
            } else {
                DISABLED.store(true, Ordering::Relaxed);
            }
        });
        KEY.load(Ordering::Acquire)
    }

    #[cold]
    fn disable(key: usize) {
        DISABLED.store(true, Ordering::Relaxed);
        KEY.store(NO_KEY, Ordering::Release);
        // SAFETY: `key` came from `pthread_key_create`.
        unsafe {
            libc::pthread_setspecific(key as libc::pthread_key_t, std::ptr::null());
        }
    }

    /// Whether the direct path is live for this process.
    ///
    /// A gate that does not assert this is measuring nothing: `false` means
    /// every `hot()` is paying `_tlv_get_addr` again and the whole change is
    /// inert. `tls_hot::tests::direct_tsd_path_is_live` is that assertion.
    pub(crate) fn active() -> bool {
        KEY.load(Ordering::Relaxed) != NO_KEY
    }
}

/// Resolve, fill and publish this thread's cache. Cold: once per thread.
#[cfg(all(
    target_vendor = "apple",
    target_arch = "aarch64",
    target_pointer_width = "64"
))]
#[cold]
#[inline(never)]
fn hot_uncached() -> &'static HotTls {
    let slots = hot_via_tls();
    darwin_tsd::publish(slots as *const HotTls as *mut u8);
    slots
}

/// The per-thread address cache. On Apple aarch64 this is an `mrs` plus two
/// loads with no call at all; elsewhere it is the single TLS access the field
/// cache already collapsed the whole allocation path down to.
#[cfg(all(
    target_vendor = "apple",
    target_arch = "aarch64",
    target_pointer_width = "64"
))]
#[inline(always)]
pub(crate) fn hot() -> &'static HotTls {
    let key = darwin_tsd::KEY.load(std::sync::atomic::Ordering::Relaxed);
    if key != darwin_tsd::NO_KEY {
        // SAFETY: `key` is a live pthread key, so the slot exists; it holds
        // either null (this thread has not published yet) or the address this
        // thread published from `HOT`.
        let slots = unsafe { darwin_tsd::get(key) } as *mut HotTls;
        if !slots.is_null() {
            // SAFETY: published from `HOT`, which is const-init with no `Drop`
            // — see the module docs on lifetime.
            return unsafe { &*slots };
        }
    }
    hot_uncached()
}

/// The per-thread address cache. One TLS access for every thread-local it
/// covers.
#[cfg(not(all(
    target_vendor = "apple",
    target_arch = "aarch64",
    target_pointer_width = "64"
)))]
#[inline(always)]
pub(crate) fn hot() -> &'static HotTls {
    hot_via_tls()
}

// ---------------------------------------------------------------------------
// Generic slots: the same collapse, without a list anyone has to maintain.
// ---------------------------------------------------------------------------

/// The process-wide slot index for one [`crate::perry_thread_local`]
/// declaration.
///
/// Claimed once, on the first thread that resolves the declaration, and stable
/// for the life of the process — so every thread finds the same declaration at
/// the same index in its own cache.
pub struct SlotId(std::sync::atomic::AtomicU32);

impl SlotId {
    pub const fn new() -> Self {
        Self(std::sync::atomic::AtomicU32::new(SLOT_UNASSIGNED))
    }

    /// The claimed index, or a sentinel `>= HOT_SLOT_CAPACITY`.
    ///
    /// Relaxed is sufficient: the index is a pure allocation decision, and the
    /// *pointer* it selects is per-thread and published by that same thread.
    #[inline(always)]
    fn raw(&self) -> u32 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Claim this declaration's index, once per process.
    ///
    /// Under a mutex rather than a bare `fetch_add` because a lost race would
    /// *leak* the index it lost with: eight `parallelMap` workers first-touching
    /// the same declaration together would burn eight slots for one declaration,
    /// and `HOT_SLOT_CAPACITY` is sized for declarations, not for declarations
    /// times threads.
    #[cold]
    #[inline(never)]
    fn claim(&self) -> u32 {
        use std::sync::atomic::Ordering;
        maybe_install_stats_hook();
        let mut next = match CLAIM_LOCK.lock() {
            Ok(next) => next,
            Err(poisoned) => poisoned.into_inner(),
        };
        let current = self.0.load(Ordering::Relaxed);
        if current != SLOT_UNASSIGNED {
            return current;
        }
        let idx = if (*next as usize) < HOT_SLOT_CAPACITY {
            let idx = *next;
            *next += 1;
            idx
        } else {
            SLOT_OVERFLOW
        };
        self.0.store(idx, Ordering::Relaxed);
        idx
    }
}

impl Default for SlotId {
    fn default() -> Self {
        Self::new()
    }
}

/// The next index [`SlotId::claim`] will hand out. Also the count of
/// declarations claimed so far, which is what
/// [`claimed_slots`] reports and what the capacity test asserts against.
static CLAIM_LOCK: std::sync::Mutex<u32> = std::sync::Mutex::new(0);

/// How many declarations have claimed a slot in this process.
///
/// Instrumentation for the capacity assertion: overflow is silent by design
/// (the declaration keeps working, slowly), so something has to be able to see
/// how close the process is to the ceiling.
pub fn claimed_slots() -> u32 {
    match CLAIM_LOCK.lock() {
        Ok(next) => *next,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

/// How many slots *this thread* has populated.
pub fn published_slots() -> usize {
    hot().slots.iter().filter(|s| !s.get().is_null()).count()
}

/// `PERRY_TLS_HOT_STATS=1` — print, at process exit, what this mechanism
/// actually did.
///
/// This exists so a budget gate can assert its subject was LIVE rather than
/// merely quiet. `_tlv_get_addr` reading 0% is the *same observation* whether
/// the cache carried the program's thread-locals or the program simply never
/// resolved one, and #7469's history is that the second case shipped as a pass
/// three times. The line reports:
///
/// * `claimed` — declarations that took a slot process-wide. A program that
///   exercises paths outside the sixteen named fields drives this well past
///   zero; a program that does not, does not.
/// * `published` — slots this thread actually filled.
/// * `direct_tsd` — whether `hot()` is the `mrs`-plus-two-loads path. `0`
///   means the self-check rejected direct addressing and every access is
///   paying `_tlv_get_addr` again, i.e. the whole mechanism is inert.
fn maybe_install_stats_hook() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        if !matches!(
            std::env::var("PERRY_TLS_HOT_STATS").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        ) {
            return;
        }
        extern "C" fn report() {
            #[cfg(all(
                target_vendor = "apple",
                target_arch = "aarch64",
                target_pointer_width = "64"
            ))]
            let direct = u8::from(darwin_tsd::active());
            #[cfg(not(all(
                target_vendor = "apple",
                target_arch = "aarch64",
                target_pointer_width = "64"
            )))]
            let direct = 0u8;
            eprintln!(
                "[tls-hot] claimed={} published={} capacity={} direct_tsd={}",
                claimed_slots(),
                published_slots(),
                HOT_SLOT_CAPACITY,
                direct,
            );
        }
        // SAFETY: `report` is `extern "C"`, takes nothing and returns nothing.
        unsafe {
            libc::atexit(report);
        }
    });
}

/// The storage a [`crate::perry_thread_local`] declaration actually owns.
///
/// `GUARD` is `needs_drop::<T>() as usize`, filled in by the macro at each
/// declaration, and it is the whole reason this is a const-generic type rather
/// than a plain struct.
///
/// * `T` owns something (`RefCell<HashMap<…>>`): `GUARD == 1`. The array's
///   element runs first — fields drop in declaration order, and `guard` is
///   declared first — so this thread's cached address is un-published *before*
///   `inner` is destroyed. A later access finds a null slot, falls back to the
///   real `thread_local!`, and gets std's "accessed during or after
///   destruction" panic instead of reading a dropped `HashMap`. The named-field
///   cache above has no such hook; this is strictly safer than it.
/// * `T` owns nothing (`Cell<u64>`): `GUARD == 0`. A zero-length array has no
///   drop glue, so `HotCell` has none either: no destructor is registered, the
///   `const`-init fast path in std is preserved, and a value that could always
///   be read during teardown still can be. Caching cannot make it dangle
///   because there is nothing to destroy.
pub struct HotCell<T: 'static, const GUARD: usize> {
    guard: [SlotGuard; GUARD],
    inner: T,
}

impl<T: 'static, const GUARD: usize> HotCell<T, GUARD> {
    pub const fn new(inner: T) -> Self {
        Self {
            guard: [const { SlotGuard::new() }; GUARD],
            inner,
        }
    }

    /// The address the cache stores: the *value*, so [`HotKey`] never has to
    /// know this type's layout — or its `GUARD`.
    #[doc(hidden)]
    pub fn value_addr(&self) -> *mut u8 {
        &self.inner as *const T as *mut u8
    }

    /// Tell the teardown guard, if there is one, which slot to un-publish.
    #[doc(hidden)]
    pub fn arm_guard(&self, idx: u32) {
        if let Some(guard) = self.guard.first() {
            guard.idx.set(idx);
        }
    }
}

/// Nulls this thread's cached pointer when the value it points into is about
/// to be destroyed.
struct SlotGuard {
    idx: Cell<u32>,
}

impl SlotGuard {
    const fn new() -> Self {
        Self {
            idx: Cell::new(SLOT_UNASSIGNED),
        }
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let idx = self.idx.get();
        if (idx as usize) < HOT_SLOT_CAPACITY {
            hot().set_slot(idx, std::ptr::null_mut());
        }
    }
}

/// A thread-local whose address is cached in this thread's [`HotTls`].
///
/// Drop-in for `std::thread::LocalKey` at the call site: `with` and `try_with`
/// keep the same signatures, so converting a declaration converts every one of
/// its uses.
pub struct HotKey<T: 'static> {
    slot: &'static SlotId,
    /// Resolves the owning `thread_local!` the ordinary way and returns the
    /// address of its *value*. Cold path only — never called once the slot is
    /// populated, so the indirect call never appears on a hot path.
    resolve: fn() -> Result<*mut u8, std::thread::AccessError>,
    /// Records the claimed index in this thread's teardown guard, if the value
    /// has one. Generated alongside the storage, so it knows the `GUARD` that
    /// `HotKey` deliberately does not.
    arm_guard: fn(u32),
    _not_send: std::marker::PhantomData<*const T>,
}

// SAFETY: exactly `LocalKey`'s argument. `HotKey` is a handle, not storage:
// every path through it resolves the *calling* thread's own cell, so no `T` is
// ever observed from a thread other than the one that owns it.
unsafe impl<T: 'static> Sync for HotKey<T> {}

impl<T: 'static> HotKey<T> {
    #[doc(hidden)]
    pub const fn new(
        slot: &'static SlotId,
        resolve: fn() -> Result<*mut u8, std::thread::AccessError>,
        arm_guard: fn(u32),
    ) -> Self {
        Self {
            slot,
            resolve,
            arm_guard,
            _not_send: std::marker::PhantomData,
        }
    }

    /// Borrow this thread's value.
    ///
    /// The fast path is: one load of the declaration's index, the [`hot`]
    /// cache (an `mrs` plus two loads on Apple aarch64, CSE'd across the whole
    /// enclosing function), one load from the slot array. No call, so no
    /// caller-saved register is clobbered at the site.
    #[inline(always)]
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(self.get())
    }

    /// As [`HotKey::with`], but reports rather than panics when this thread's
    /// value is being or has been destroyed.
    #[inline(always)]
    pub fn try_with<F, R>(&'static self, f: F) -> Result<R, std::thread::AccessError>
    where
        F: FnOnce(&T) -> R,
    {
        let idx = self.slot.raw();
        if (idx as usize) < HOT_SLOT_CAPACITY {
            let cell = hot().slot(idx);
            if !cell.is_null() {
                // SAFETY: see `value_of`.
                return Ok(f(unsafe { Self::value_of(cell) }));
            }
        }
        let cell = self.resolve_and_cache()?;
        // SAFETY: see `value_of`.
        Ok(f(unsafe { Self::value_of(cell) }))
    }

    #[inline(always)]
    fn get(&'static self) -> &'static T {
        let idx = self.slot.raw();
        if (idx as usize) < HOT_SLOT_CAPACITY {
            let cell = hot().slot(idx);
            if !cell.is_null() {
                // SAFETY: see `value_of`.
                return unsafe { Self::value_of(cell) };
            }
        }
        self.get_slow()
    }

    #[cold]
    #[inline(never)]
    fn get_slow(&'static self) -> &'static T {
        let cell = self
            .resolve_and_cache()
            .expect("cannot access a Perry thread-local during or after thread destruction");
        // SAFETY: see `value_of`.
        unsafe { Self::value_of(cell) }
    }

    /// This declaration's claimed slot index, or a sentinel
    /// `>= HOT_SLOT_CAPACITY` if it has none. Liveness instrumentation: a test
    /// that does not check this passes identically when the cache is inert.
    #[doc(hidden)]
    pub fn slot_index(&'static self) -> u32 {
        self.slot.raw()
    }

    /// `value` is the address of this thread's `T`, published by this key.
    ///
    /// # Safety
    /// `value` must have come from this key's slot or from its own `resolve`.
    /// That is what makes the cross-cast the module docs warn about impossible
    /// here: [`crate::perry_thread_local`] generates the storage, the resolver
    /// and this key's `T` from one declaration, so there is no hand-written
    /// pairing left to get wrong.
    #[inline(always)]
    unsafe fn value_of(value: *mut u8) -> &'static T {
        // SAFETY: the caller guarantees provenance; the value outlives this
        // thread's use of it (see the `SlotGuard` note on destruction).
        unsafe { &*(value as *const T) }
    }

    /// Resolve through the real `thread_local!`, claim this declaration's slot
    /// if it has none yet, and publish the address for this thread.
    #[cold]
    #[inline(never)]
    fn resolve_and_cache(&'static self) -> Result<*mut u8, std::thread::AccessError> {
        // Resolve first, and outside the claim lock: initialising the value can
        // run arbitrary runtime code, including other `perry_thread_local!`
        // first touches.
        let value = (self.resolve)()?;
        let mut idx = self.slot.raw();
        if idx == SLOT_UNASSIGNED {
            idx = self.slot.claim();
        }
        if (idx as usize) < HOT_SLOT_CAPACITY {
            // Arm before publishing: after this store any thread-teardown of
            // the value un-publishes the slot it is about to invalidate.
            (self.arm_guard)(idx);
            hot().set_slot(idx, value);
        }
        Ok(value)
    }
}

/// Declare a thread-local that is on the fast path **by default**.
///
/// Same syntax as `std::thread_local!`, same `with` / `try_with` at every call
/// site — the only difference is that the address of the value lands in this
/// thread's [`HotTls`] cache, so reads cost loads instead of a `_tlv_get_addr`
/// call on Darwin.
///
/// # Why this is the default rather than an allowlist
///
/// The named fields at the top of this file are an opt-in list of sixteen,
/// curated against whichever workload was profiled last. Every hot path the
/// list did not anticipate silently paid full price: `churn` read 0% of
/// `_tlv_get_addr` for months while `asyncpipe` — Map/Set registries, buffer
/// brands, descriptor state, none of them on the list — paid 20.5%. Adding a
/// field took four manual steps including a hand-written test, which does not
/// scale to ~520 declarations and, worse, gets the *default* wrong: forgetting
/// the steps produces a working slow path rather than a build error.
///
/// Here there is nothing to wire. The declaration generates its own storage,
/// its own resolver and its own [`HotKey`], so a mis-pairing of slot and type —
/// the hazard the untyped named slots need `cached_addresses_match_thread_locals`
/// to catch — cannot be expressed.
///
/// # Forms
///
/// ```ignore
/// crate::perry_thread_local! {
///     static COUNTER: Cell<u64> = const { Cell::new(0) };
///     static REGISTRY: RefCell<HashMap<usize, u32>> = RefCell::new(HashMap::new());
/// }
/// ```
///
/// Both forms behave exactly as `std::thread_local!`'s do, including whether a
/// destructor is registered: the teardown guard is present iff
/// `needs_drop::<T>()`, so a `Cell<u64>` keeps std's drop-free `const` path and
/// a `RefCell<HashMap<…>>` gets un-published before it is destroyed.
#[macro_export]
macro_rules! perry_thread_local {
    () => {};

    // `= const { ... }`
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = const $init:block; $($rest:tt)*) => {
        $crate::__perry_thread_local_one! { $(#[$attr])* $vis $name, $t, const $init }
        $crate::perry_thread_local!($($rest)*);
    };

    // `= expr`
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr; $($rest:tt)*) => {
        $crate::__perry_thread_local_one! { $(#[$attr])* $vis $name, $t, expr ($init) }
        $crate::perry_thread_local!($($rest)*);
    };
}

/// One declaration. Split out only so the two init forms share everything but
/// the line that hands the initialiser to `std::thread_local!`.
#[doc(hidden)]
#[macro_export]
macro_rules! __perry_thread_local_one {
    ($(#[$attr:meta])* $vis:vis $name:ident, $t:ty, $($init:tt)+) => {
        $(#[$attr])*
        $vis static $name: $crate::tls_hot::HotKey<$t> = {
            static SLOT: $crate::tls_hot::SlotId = $crate::tls_hot::SlotId::new();
            // `GUARD` is 1 exactly when `$t` has drop glue, so the guard —
            // and with it the thread-local's destructor — exists exactly when
            // a cached address could otherwise outlive the value.
            type Storage = $crate::tls_hot::HotCell<$t, { ::core::mem::needs_drop::<$t>() as usize }>;
            $crate::__perry_thread_local_storage!(Storage, $($init)+);
            fn resolve() -> ::core::result::Result<*mut u8, ::std::thread::AccessError> {
                STORAGE.try_with(|cell| cell.value_addr())
            }
            fn arm_guard(idx: u32) {
                let _ = STORAGE.try_with(|cell| cell.arm_guard(idx));
            }
            $crate::tls_hot::HotKey::new(&SLOT, resolve, arm_guard)
        };
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __perry_thread_local_storage {
    ($storage:ty, const $init:block) => {
        ::std::thread_local! {
            static STORAGE: $storage = const { <$storage>::new($init) };
        }
    };
    ($storage:ty, expr ($init:expr)) => {
        ::std::thread_local! {
            static STORAGE: $storage = <$storage>::new($init);
        }
    };
}

#[cfg(test)]
mod tests {
    /// Every cached address must equal the address of the `thread_local!` it
    /// mirrors. The slots are untyped, so this is what stands between a
    /// mis-wired [`super::fill`] and a well-typed reference to the wrong
    /// object.
    #[test]
    fn cached_addresses_match_thread_locals() {
        let hot = super::hot();
        assert_eq!(hot.arena, crate::arena::arena_hot_addr(), "arena");
        assert_eq!(
            hot.inline_state,
            crate::arena::inline_state_hot_addr(),
            "inline_state"
        );
        assert_eq!(
            hot.page_generation_cache,
            crate::arena::page_generation_cache_hot_addr(),
            "page_generation_cache"
        );
        assert_eq!(
            hot.page_generations,
            crate::arena::page_generations_hot_addr(),
            "page_generations"
        );
        assert_eq!(
            hot.arena_free_list,
            crate::gc::arena_free_list_hot_addr(),
            "arena_free_list"
        );
        assert_eq!(
            hot.arena_free_list_nonempty,
            crate::gc::arena_free_list_nonempty_hot_addr(),
            "arena_free_list_nonempty"
        );
        assert_eq!(
            hot.birth_extra_flags,
            crate::gc::birth_extra_flags_hot_addr(),
            "birth_extra_flags"
        );
        assert_eq!(
            hot.incremental_mark_valid_ptrs,
            crate::gc::incremental_mark_valid_ptrs_hot_addr(),
            "incremental_mark_valid_ptrs"
        );
        assert_eq!(
            hot.incremental_mark_minor_only,
            crate::gc::incremental_mark_minor_only_hot_addr(),
            "incremental_mark_minor_only"
        );
        assert_eq!(
            hot.layout_slot_masks,
            crate::gc::layout_slot_masks_hot_addr(),
            "layout_slot_masks"
        );
        assert_eq!(
            hot.typed_layouts,
            crate::gc::typed_layouts_hot_addr(),
            "typed_layouts"
        );
        assert_eq!(
            hot.shape_layouts,
            crate::gc::shape_layouts_hot_addr(),
            "shape_layouts"
        );
        assert_eq!(
            hot.per_object_layouts_nonempty,
            crate::gc::per_object_layouts_nonempty_hot_addr(),
            "per_object_layouts_nonempty"
        );
        assert_eq!(
            hot.shape_install_memo,
            crate::gc::shape_install_memo_hot_addr(),
            "shape_install_memo"
        );
        assert_eq!(
            hot.learned_inline_fields,
            crate::object::learned_inline_fields_hot_addr(),
            "learned_inline_fields"
        );
        assert_eq!(
            hot.temp_roots,
            crate::gc::temp_roots_hot_addr(),
            "temp_roots"
        );
    }

    /// No slot may be null after `hot()` — a null would mean `fill` skipped a
    /// provider, and the typed accessor would dereference it.
    #[test]
    fn every_slot_is_populated() {
        let hot = super::hot();
        for (name, ptr) in [
            ("arena", hot.arena),
            ("inline_state", hot.inline_state),
            ("page_generation_cache", hot.page_generation_cache),
            ("page_generations", hot.page_generations),
            ("arena_free_list", hot.arena_free_list),
            ("arena_free_list_nonempty", hot.arena_free_list_nonempty),
            ("birth_extra_flags", hot.birth_extra_flags),
            (
                "incremental_mark_valid_ptrs",
                hot.incremental_mark_valid_ptrs,
            ),
            (
                "incremental_mark_minor_only",
                hot.incremental_mark_minor_only,
            ),
            ("layout_slot_masks", hot.layout_slot_masks),
            ("typed_layouts", hot.typed_layouts),
            ("shape_layouts", hot.shape_layouts),
            (
                "per_object_layouts_nonempty",
                hot.per_object_layouts_nonempty,
            ),
            ("shape_install_memo", hot.shape_install_memo),
            ("learned_inline_fields", hot.learned_inline_fields),
            ("temp_roots", hot.temp_roots),
        ] {
            assert!(!ptr.is_null(), "{name} slot was left null by fill()");
        }
    }

    /// The direct thread-specific-data path must be live on this platform.
    ///
    /// This is the liveness assertion for #7469's structural half. Every other
    /// test here passes identically whether `hot()` costs an `_tlv_get_addr`
    /// call or three inline instructions, so without this one a silent fallback
    /// would make the change inert and nothing would go red.
    #[cfg(all(
        target_vendor = "apple",
        target_arch = "aarch64",
        target_pointer_width = "64"
    ))]
    #[test]
    fn direct_tsd_path_is_live() {
        let expected = super::hot() as *const super::HotTls;
        assert!(
            super::darwin_tsd::active(),
            "direct TSD path fell back to _tlv_get_addr — the #7469 structural \
             fix is inert on this run"
        );
        let key = super::darwin_tsd::KEY.load(std::sync::atomic::Ordering::Relaxed);
        // SAFETY: `active()` above proves the key came from pthread_key_create.
        let direct = unsafe { super::darwin_tsd::get(key) } as *const super::HotTls;
        assert_eq!(
            direct, expected,
            "direct TSD read disagreed with the published cache address"
        );
    }

    /// The direct read must agree with `pthread_getspecific` for the same key.
    ///
    /// `get()` open-codes what libpthread does; this is the check that says so
    /// against the real implementation rather than against our own belief about
    /// it. If Apple ever moves the TSD array, this fails here rather than in
    /// the allocator.
    #[cfg(all(
        target_vendor = "apple",
        target_arch = "aarch64",
        target_pointer_width = "64"
    ))]
    #[test]
    fn direct_read_matches_pthread_getspecific() {
        let mut key: libc::pthread_key_t = 0;
        // SAFETY: `key` is a live local for the duration of the call.
        assert_eq!(unsafe { libc::pthread_key_create(&mut key, None) }, 0);
        let sentinel = 0x5eed_1234_usize as *mut libc::c_void;
        // SAFETY: `key` was just created.
        assert_eq!(
            unsafe { libc::pthread_setspecific(key, sentinel) },
            0,
            "pthread_setspecific rejected a freshly created key"
        );
        // SAFETY: as above.
        let direct = unsafe { super::darwin_tsd::get(key as usize) };
        // SAFETY: as above.
        let via_pthread = unsafe { libc::pthread_getspecific(key) };
        assert_eq!(direct as *mut libc::c_void, via_pthread);
        assert_eq!(direct, sentinel.cast());
        // SAFETY: as above.
        unsafe {
            libc::pthread_setspecific(key, std::ptr::null());
            libc::pthread_key_delete(key);
        }
    }

    /// A thread that has not published yet must see null in its slot and take
    /// the cold path, not another thread's cache.
    #[cfg(all(
        target_vendor = "apple",
        target_arch = "aarch64",
        target_pointer_width = "64"
    ))]
    #[test]
    fn a_fresh_thread_publishes_its_own_slot() {
        let mine = super::hot() as *const super::HotTls;
        let theirs = std::thread::spawn(|| {
            let addr = super::hot() as *const super::HotTls;
            let key = super::darwin_tsd::KEY.load(std::sync::atomic::Ordering::Relaxed);
            assert_ne!(key, super::darwin_tsd::NO_KEY);
            // SAFETY: `key` is live; the assert above proves it was created.
            let direct = unsafe { super::darwin_tsd::get(key) } as *const super::HotTls;
            assert_eq!(direct, addr, "worker thread published the wrong address");
            addr as usize
        })
        .join()
        .expect("probe thread panicked");
        assert_ne!(mine as usize, theirs, "two threads shared one cache");
    }

    /// The cache is per-thread: a second thread must resolve its own
    /// addresses, not inherit this one's.
    #[test]
    fn each_thread_caches_its_own_addresses() {
        let mine = super::hot().temp_roots as usize;
        let theirs = std::thread::spawn(|| super::hot().temp_roots as usize)
            .join()
            .expect("probe thread panicked");
        assert_ne!(
            mine, theirs,
            "two threads resolved the same temp-root address"
        );
    }

    // -----------------------------------------------------------------
    // Generic slots
    // -----------------------------------------------------------------

    crate::perry_thread_local! {
        static PROBE_CONST: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        static PROBE_EXPR: std::cell::RefCell<Vec<u64>> = std::cell::RefCell::new(Vec::new());
        static PROBE_SECOND: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    /// The address a `perry_thread_local!` hands out is this thread's real
    /// storage, and it is stable across accesses.
    ///
    /// The named slots need `cached_addresses_match_thread_locals` because a
    /// human writes the pairing; here the storage, the resolver and the key's
    /// `T` all come from one declaration, so this is a liveness check (the slot
    /// really is being used) rather than a correctness one.
    #[test]
    fn a_generic_slot_is_this_threads_storage() {
        PROBE_CONST.with(|c| c.set(0x5eed));
        let first = PROBE_CONST.with(|c| c as *const _ as usize);
        let second = PROBE_CONST.with(|c| c as *const _ as usize);
        assert_eq!(first, second, "the cached address moved between accesses");
        assert_eq!(PROBE_CONST.with(|c| c.get()), 0x5eed);

        // Liveness: the access above must actually have gone through a slot.
        // Every other assertion in this module passes identically whether the
        // cache is used or every read falls back to `_tlv_get_addr`, so without
        // this one the whole mechanism could be inert and nothing would go red.
        let idx = PROBE_CONST.slot_index();
        assert!(
            (idx as usize) < super::HOT_SLOT_CAPACITY,
            "the declaration never claimed a slot (idx {idx})",
        );
        assert_eq!(
            super::hot().slot(idx) as usize,
            first,
            "the slot does not hold the address `with` handed out",
        );
    }

    /// Distinct declarations must never share a slot: that is the one way a
    /// generic slot could hand out a correctly-typed reference to the wrong
    /// object, which is the hazard the module docs are about.
    #[test]
    fn distinct_declarations_do_not_share_a_slot() {
        PROBE_CONST.with(|c| c.set(11));
        PROBE_SECOND.with(|c| c.set(22));
        assert_eq!(PROBE_CONST.with(|c| c.get()), 11);
        assert_eq!(PROBE_SECOND.with(|c| c.get()), 22);
        let a = PROBE_CONST.with(|c| c as *const _ as usize);
        let b = PROBE_SECOND.with(|c| c as *const _ as usize);
        assert_ne!(a, b, "two declarations resolved to one address");
    }

    /// Each thread resolves its own storage, and a worker's slot must not
    /// leak into the parent's cache.
    #[test]
    fn a_generic_slot_is_per_thread() {
        PROBE_EXPR.with(|v| v.borrow_mut().push(1));
        let mine = PROBE_EXPR.with(|v| v as *const _ as usize);
        let theirs = std::thread::spawn(|| {
            PROBE_EXPR.with(|v| {
                assert!(
                    v.borrow().is_empty(),
                    "a worker inherited the parent thread's value"
                );
                v.borrow_mut().push(2);
            });
            PROBE_EXPR.with(|v| v as *const _ as usize)
        })
        .join()
        .expect("probe thread panicked");
        assert_ne!(mine, theirs, "two threads shared one slot's storage");
        assert_eq!(PROBE_EXPR.with(|v| v.borrow().clone()), vec![1]);
    }

    /// Hammer the claim path from many threads at once: a lost race that
    /// leaked an index would show up as capacity draining far past the number
    /// of declarations.
    #[test]
    fn concurrent_first_touch_claims_one_slot_per_declaration() {
        let before = super::claimed_slots();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    PROBE_CONST.with(|c| c.get());
                    PROBE_EXPR.with(|v| v.borrow().len());
                    PROBE_SECOND.with(|c| c.get());
                })
            })
            .collect();
        for w in workers {
            w.join().expect("probe thread panicked");
        }
        let after = super::claimed_slots();
        assert!(
            after - before <= 3,
            "8 threads first-touching 3 declarations claimed {} slots",
            after - before
        );
        assert!(
            (after as usize) < super::HOT_SLOT_CAPACITY,
            "slot capacity {} exhausted at {after} claims",
            super::HOT_SLOT_CAPACITY
        );
    }

    /// A value with a destructor must stop being served from the cache the
    /// moment its thread starts destroying it.
    ///
    /// The ordering is the point. std runs thread-local destructors in reverse
    /// registration order, so touching `AFTER_PROBE` *first* and `PROBE_EXPR`
    /// second puts `PROBE_EXPR`'s destructor ahead of `AFTER_PROBE`'s: by the
    /// time `AFTER_PROBE` drops and re-reads the key, the guard must already
    /// have un-published it. Without the guard this test reads a dropped `Vec`
    /// — the exact use-after-free the named-field cache has no defence against.
    #[test]
    fn teardown_unpublishes_a_dropping_value() {
        use std::sync::atomic::{AtomicU8, Ordering};
        static OBSERVED: AtomicU8 = AtomicU8::new(0);
        const UNSEEN: u8 = 0;
        const REPORTED_DESTROYED: u8 = 1;
        const SERVED_STALE: u8 = 2;

        struct AfterProbe;
        impl Drop for AfterProbe {
            fn drop(&mut self) {
                let state = match PROBE_EXPR.try_with(|v| v.borrow().len()) {
                    Ok(_) => SERVED_STALE,
                    Err(_) => REPORTED_DESTROYED,
                };
                OBSERVED.store(state, Ordering::SeqCst);
            }
        }
        thread_local! {
            static AFTER_PROBE: AfterProbe = const { AfterProbe };
        }

        std::thread::spawn(|| {
            // Registration order: AFTER_PROBE, then PROBE_EXPR's storage.
            AFTER_PROBE.with(|_| {});
            PROBE_EXPR.with(|v| v.borrow_mut().push(7));
        })
        .join()
        .expect("probe thread panicked");

        assert_eq!(
            OBSERVED.load(Ordering::SeqCst),
            REPORTED_DESTROYED,
            "a destroyed thread-local was still served from the hot cache \
             (0 = the probe never ran, 2 = it read the dropped value)",
        );
        assert_ne!(OBSERVED.load(Ordering::SeqCst), UNSEEN);
    }

    /// Real converted declarations, across many short-lived threads.
    ///
    /// `perry/thread`'s `spawn` and `parallelMap` run JS on OS threads with
    /// their own arenas, so every converted declaration is resolved, published
    /// and destroyed once per worker. This drives that cycle 64 times over the
    /// registry probes the profiles named — the ones `asyncpipe` and `interp`
    /// spend their `_tlv_get_addr` on — and asserts both that nothing faults
    /// and that repeated thread turnover cannot drain slots: an index is
    /// claimed per *declaration*, not per thread.
    #[test]
    fn converted_declarations_survive_thread_turnover() {
        fn touch_the_converted_registries() -> usize {
            let mut seen = 0;
            for probe in [0usize, 1, 0x1000, usize::MAX / 2] {
                seen += usize::from(crate::map::is_registered_map(probe));
                seen += usize::from(crate::set::is_registered_set(probe));
                seen += usize::from(crate::buffer::is_registered_buffer(probe));
                seen += usize::from(crate::symbol::is_registered_symbol(probe));
                seen += usize::from(crate::regex::is_regex_pointer(probe as *const u8));
            }
            seen
        }

        touch_the_converted_registries();
        let after_main = super::claimed_slots();

        for _ in 0..8 {
            let batch: Vec<_> = (0..8)
                .map(|_| std::thread::spawn(touch_the_converted_registries))
                .collect();
            for worker in batch {
                worker.join().expect("worker thread panicked");
            }
        }

        let after_workers = super::claimed_slots();
        // A TOLERANCE, not equality, and the reason is the measurement rather
        // than the mechanism: `claimed_slots()` is PROCESS-global, so any other
        // test in this binary that touches a converted declaration for the
        // first time lands a claim inside this window. Under `--test-threads=1`
        // the delta is exactly 0; in parallel it is 0 or a stray 1-2 from a
        // neighbour. Asserting equality made this fail 6/6 under load while
        // passing 3/3 in isolation and 1/1 single-threaded.
        //
        // The tolerance still separates the two outcomes by two orders of
        // magnitude: per-THREAD claiming — the bug this test exists to catch —
        // would add 5 declarations x 64 workers = 320, not 1.
        const TURNOVER_CLAIM_TOLERANCE: u32 = 8;
        let extra = after_workers.saturating_sub(after_main);
        assert!(
            extra <= TURNOVER_CLAIM_TOLERANCE,
            "64 worker threads claimed {extra} extra slots (tolerance \
             {TURNOVER_CLAIM_TOLERANCE}); indices must be per declaration, not \
             per thread — per-thread claiming would add 5 x 64 = 320",
        );
        assert!(
            (after_workers as usize) < super::HOT_SLOT_CAPACITY,
            "capacity {} exhausted at {after_workers}",
            super::HOT_SLOT_CAPACITY,
        );
        // And the parent thread's own cache is still serving after all that.
        assert_eq!(touch_the_converted_registries(), 0);
    }

    /// The guard exists exactly when the value has something to destroy — that
    /// is what keeps drop-free declarations off std's destructor path.
    #[test]
    fn the_guard_tracks_drop_glue() {
        assert_eq!(
            std::mem::size_of::<super::HotCell<std::cell::Cell<u64>, 0>>(),
            std::mem::size_of::<std::cell::Cell<u64>>(),
            "a drop-free declaration paid for a guard",
        );
        assert!(!std::mem::needs_drop::<
            super::HotCell<std::cell::Cell<u64>, 0>,
        >());
        assert!(std::mem::needs_drop::<
            super::HotCell<std::cell::RefCell<Vec<u64>>, 1>,
        >());
    }
}
