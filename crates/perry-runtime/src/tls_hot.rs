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
//! # Contract for adding a field
//!
//! 1. Add the `*mut u8` slot below.
//! 2. Add a `pub(crate) fn …_hot_addr() -> *mut u8` next to the `thread_local!`
//!    that owns the storage, returning `KEY.with(|k| k as *const _ as *mut u8)`.
//! 3. Wire it in [`fill`].
//! 4. Add the pair to `tls_hot::tests::cached_addresses_match_thread_locals`.
//!
//! Step 4 is the load-bearing one: the slots are untyped (`*mut u8`) so the
//! owning module can keep its storage type private, which means a mis-wired
//! `fill` would hand out a correctly-typed reference to the *wrong* object.
//! The test compares each cached address against the `.with()` address it is
//! supposed to mirror, so a mis-wire is a red build rather than a silent
//! cross-cast.
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

use std::cell::UnsafeCell;

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
    // gc/roots/temp_roots.rs
    pub(crate) temp_roots: *mut u8,
}

impl HotTls {
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
        temp_roots: std::ptr::null_mut(),
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
    #[inline(always)]
    pub(super) unsafe fn get(slot: usize) -> *mut u8 {
        let base: usize;
        // `pure` + `nomem` is deliberate. Stripped of its CPU-number bits the
        // register is a per-thread constant, so LLVM may CSE it across a whole
        // function and hoist it out of loops. Without `pure` every reader
        // re-issues the `mrs` and the change buys far less than the call it
        // replaces.
        core::arch::asm!(
            "mrs {b}, tpidrro_el0",
            b = out(reg) base,
            options(nomem, nostack, preserves_flags, pure)
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
}
