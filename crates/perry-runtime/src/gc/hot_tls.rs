//! The `gc` half of the #7469 hot-thread-local plumbing.
//!
//! Two kinds of function live here, one pair per thread-local:
//!
//! - `…_hot_addr()` — resolves the thread-local's address the ordinary way.
//!   Called once per thread by [`crate::tls_hot::fill`], which lives outside
//!   `gc` and so needs these re-exported at `pub(crate)`.
//! - `hot_…()` — reads the cached address back and casts it to the owning
//!   type. This is what the hot paths call instead of `KEY.with(…)`, and it is
//!   why a single `{v, w}` object literal no longer pays a `_tlv_get_addr`
//!   call per side table it touches.
//!
//! The casts are the reason the pairs sit together in one file: the cache
//! stores untyped `*mut u8` (so each owning module keeps its storage type
//! private), so a `hot_…()` paired with the wrong `…_hot_addr()` would hand
//! out a well-typed reference to the wrong object.
//! `tls_hot::tests::cached_addresses_match_thread_locals` asserts every pairing
//! and is the guard against exactly that.
//!
//! Split out of `barrier.rs` and `layout.rs` to stay under the repo's
//! 2000-line-per-file cap (`scripts/check_file_size.sh`).

use super::barrier::{
    GC_BIRTH_EXTRA_FLAGS, INCREMENTAL_MARK_BARRIER_MINOR_ONLY, INCREMENTAL_MARK_BARRIER_VALID_PTRS,
};
use super::layout::{LayoutSlotMask, TypedLayoutDescriptor, SHAPE_LAYOUTS};
use super::layout_tables::{LAYOUT_SLOT_MASKS, PER_OBJECT_LAYOUTS_NONEMPTY, TYPED_LAYOUTS};
use super::malloc::{ARENA_FREE_LIST, ARENA_FREE_LIST_NONEMPTY};
use super::trace::ValidPointerSet;
use std::cell::{Cell, RefCell};

// --- gc::barrier ------------------------------------------------------------

/// Address of this thread's `GC_BIRTH_EXTRA_FLAGS`.
pub(crate) fn birth_extra_flags_hot_addr() -> *mut u8 {
    GC_BIRTH_EXTRA_FLAGS.with(|c| c as *const _ as *mut u8)
}

/// Address of this thread's `INCREMENTAL_MARK_BARRIER_VALID_PTRS`.
pub(crate) fn incremental_mark_valid_ptrs_hot_addr() -> *mut u8 {
    INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|c| c as *const _ as *mut u8)
}

/// Address of this thread's `INCREMENTAL_MARK_BARRIER_MINOR_ONLY`.
pub(crate) fn incremental_mark_minor_only_hot_addr() -> *mut u8 {
    INCREMENTAL_MARK_BARRIER_MINOR_ONLY.with(|c| c as *const _ as *mut u8)
}

/// `GC_BIRTH_EXTRA_FLAGS` without a TLS resolution.
#[inline(always)]
pub(super) fn hot_birth_extra_flags() -> &'static Cell<u8> {
    // SAFETY: paired with `birth_extra_flags_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().birth_extra_flags as *const Cell<u8>) }
}

/// `INCREMENTAL_MARK_BARRIER_VALID_PTRS` without a TLS resolution.
#[inline(always)]
pub(super) fn hot_incremental_mark_valid_ptrs() -> &'static Cell<*const ValidPointerSet> {
    // SAFETY: paired with `incremental_mark_valid_ptrs_hot_addr` above.
    unsafe {
        &*(crate::tls_hot::hot().incremental_mark_valid_ptrs as *const Cell<*const ValidPointerSet>)
    }
}

/// `INCREMENTAL_MARK_BARRIER_MINOR_ONLY` without a TLS resolution.
#[inline(always)]
pub(super) fn hot_incremental_mark_minor_only() -> &'static Cell<bool> {
    // SAFETY: paired with `incremental_mark_minor_only_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().incremental_mark_minor_only as *const Cell<bool>) }
}

// --- gc::layout -------------------------------------------------------------

type SlotMaskMap = crate::fast_hash::PtrHashMap<usize, LayoutSlotMask>;
type TypedLayoutMap = crate::fast_hash::PtrHashMap<usize, TypedLayoutDescriptor>;
type ShapeLayoutMap = crate::fast_hash::PtrHashMap<usize, Option<TypedLayoutDescriptor>>;

/// Address of this thread's `LAYOUT_SLOT_MASKS`.
pub(crate) fn layout_slot_masks_hot_addr() -> *mut u8 {
    LAYOUT_SLOT_MASKS.with(|m| m as *const _ as *mut u8)
}

/// Address of this thread's `TYPED_LAYOUTS`.
pub(crate) fn typed_layouts_hot_addr() -> *mut u8 {
    TYPED_LAYOUTS.with(|m| m as *const _ as *mut u8)
}

/// Address of this thread's `SHAPE_LAYOUTS`.
pub(crate) fn shape_layouts_hot_addr() -> *mut u8 {
    SHAPE_LAYOUTS.with(|m| m as *const _ as *mut u8)
}

/// Address of this thread's `PER_OBJECT_LAYOUTS_NONEMPTY`.
pub(crate) fn per_object_layouts_nonempty_hot_addr() -> *mut u8 {
    PER_OBJECT_LAYOUTS_NONEMPTY.with(|c| c as *const _ as *mut u8)
}

/// `LAYOUT_SLOT_MASKS` without a TLS resolution.
#[inline(always)]
pub(super) fn hot_layout_slot_masks() -> &'static RefCell<SlotMaskMap> {
    // SAFETY: paired with `layout_slot_masks_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().layout_slot_masks as *const RefCell<SlotMaskMap>) }
}

/// `TYPED_LAYOUTS` without a TLS resolution.
#[inline(always)]
pub(super) fn hot_typed_layouts() -> &'static RefCell<TypedLayoutMap> {
    // SAFETY: paired with `typed_layouts_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().typed_layouts as *const RefCell<TypedLayoutMap>) }
}

/// `SHAPE_LAYOUTS` without a TLS resolution.
#[inline(always)]
pub(super) fn hot_shape_layouts() -> &'static RefCell<ShapeLayoutMap> {
    // SAFETY: paired with `shape_layouts_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().shape_layouts as *const RefCell<ShapeLayoutMap>) }
}

/// `PER_OBJECT_LAYOUTS_NONEMPTY` without a TLS resolution — the "is there any
/// per-object layout record at all" question the allocation, store, death and
/// trace paths all ask (#7510).
#[inline(always)]
pub(super) fn hot_per_object_layouts_nonempty() -> &'static Cell<bool> {
    // SAFETY: paired with `per_object_layouts_nonempty_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().per_object_layouts_nonempty as *const Cell<bool>) }
}

// --- #7598: parse-cohort birth window ---------------------------------------

thread_local! {
    /// Nesting depth of [`ParseBirthOldScope`] windows on this thread. While
    /// nonzero, `arena_alloc_gc` births go straight to old-gen with
    /// `GC_FLAG_TENURED` — the JSON tape materialiser arms it around each
    /// cohort re-parse, so the records AND their whole subtrees (strings,
    /// sub-objects, element backings — everything funnels through
    /// `arena_alloc_gc`) are born together in the generation they will live
    /// in. `u32` depth rather than bool: `reparse_materialize` is reachable
    /// from inside stringify's force-materialize, so windows nest.
    static PARSE_BIRTH_OLD_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Address of this thread's `PARSE_BIRTH_OLD_DEPTH`.
pub(crate) fn parse_birth_old_depth_hot_addr() -> *mut u8 {
    PARSE_BIRTH_OLD_DEPTH.with(|f| f as *const _ as *mut u8)
}

/// `PARSE_BIRTH_OLD_DEPTH` without a TLS resolution — the "is a parse-cohort
/// birth window open?" probe `arena_alloc_gc` pays on every allocation. Same
/// cached-base pattern (and cost class) as `hot_arena_free_list_nonempty`,
/// which that function already reads.
#[inline(always)]
pub(crate) fn hot_parse_birth_old_depth() -> &'static Cell<u32> {
    // SAFETY: paired with `parse_birth_old_depth_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().parse_birth_old_depth as *const Cell<u32>) }
}

/// #7598: RAII window — allocations on this thread are born tenured in
/// old-gen while any scope is alive. See `PARSE_BIRTH_OLD_DEPTH`.
pub(crate) struct ParseBirthOldScope;

impl ParseBirthOldScope {
    pub(crate) fn new() -> Self {
        let cell = hot_parse_birth_old_depth();
        cell.set(cell.get() + 1);
        Self
    }
}

impl Drop for ParseBirthOldScope {
    fn drop(&mut self) {
        let cell = hot_parse_birth_old_depth();
        cell.set(cell.get().saturating_sub(1));
    }
}

// --- gc::malloc -------------------------------------------------------------

/// Address of this thread's `ARENA_FREE_LIST`.
pub(crate) fn arena_free_list_hot_addr() -> *mut u8 {
    ARENA_FREE_LIST.with(|f| f as *const _ as *mut u8)
}

/// Address of this thread's `ARENA_FREE_LIST_NONEMPTY`.
pub(crate) fn arena_free_list_nonempty_hot_addr() -> *mut u8 {
    ARENA_FREE_LIST_NONEMPTY.with(|f| f as *const _ as *mut u8)
}

/// `ARENA_FREE_LIST` without a TLS resolution.
#[inline(always)]
pub(crate) fn hot_arena_free_list() -> &'static RefCell<Vec<(*mut u8, usize)>> {
    // SAFETY: paired with `arena_free_list_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().arena_free_list as *const RefCell<Vec<(*mut u8, usize)>>) }
}

/// `ARENA_FREE_LIST_NONEMPTY` without a TLS resolution — the "is there anything
/// to reuse?" probe every `arena_alloc_gc` pays.
#[inline(always)]
pub(crate) fn hot_arena_free_list_nonempty() -> &'static Cell<bool> {
    // SAFETY: paired with `arena_free_list_nonempty_hot_addr` above.
    unsafe { &*(crate::tls_hot::hot().arena_free_list_nonempty as *const Cell<bool>) }
}
