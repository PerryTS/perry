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

/// The per-thread address cache. One `_tlv_get_addr` for every thread-local it
/// covers.
#[inline(always)]
pub(crate) fn hot() -> &'static HotTls {
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
