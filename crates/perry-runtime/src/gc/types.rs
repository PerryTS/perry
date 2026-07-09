/// GC header prepended to every heap allocation.
/// Callers receive a pointer AFTER this header (ptr + 8).
#[repr(C)]
pub struct GcHeader {
    /// GC_TYPE_ARRAY, GC_TYPE_STRING, etc.
    pub obj_type: u8,
    /// GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED
    pub gc_flags: u8,
    /// Reserved for future use
    pub _reserved: u16,
    /// Total allocation size (header + payload) for arena block walking
    pub size: u32,
}

pub const GC_HEADER_SIZE: usize = std::mem::size_of::<GcHeader>(); // 8 bytes

// Object type constants
pub const GC_TYPE_ARRAY: u8 = 1;
pub const GC_TYPE_OBJECT: u8 = 2;
pub const GC_TYPE_STRING: u8 = 3;
pub const GC_TYPE_CLOSURE: u8 = 4;
pub const GC_TYPE_PROMISE: u8 = 5;
pub const GC_TYPE_BIGINT: u8 = 6;
pub const GC_TYPE_ERROR: u8 = 7;
pub const GC_TYPE_MAP: u8 = 8;
/// Issue #179 Step 2 Phase 2: lazy JSON-parse top-level array.
/// Arena-allocated, same fast-alloc path as regular arrays.
/// `js_array_length` and `js_json_stringify` recognize this type and
/// serve reads / stringify directly from the tape + blob bytes
/// without materializing the tree. Any other accessor
/// force-materializes (mutates the header's `materialized` field so
/// subsequent accesses hit the tree).
pub const GC_TYPE_LAZY_ARRAY: u8 = 9;
pub const GC_TYPE_BUFFER: u8 = 10;
pub const GC_TYPE_TYPED_ARRAY: u8 = 11;
pub const GC_TYPE_SET: u8 = 12;
pub const GC_TYPE_NATIVE_ARENA_OWNER: u8 = 13;
pub const GC_TYPE_NATIVE_TYPED_VIEW: u8 = 14;
pub const GC_TYPE_NATIVE_HANDLE: u8 = 15;
pub const GC_TYPE_NATIVE_POD_VIEW: u8 = 16;
/// A 1-slot mutable `Date` cell (`DateCell { ts: f64 }`). Arena-allocated,
/// non-movable (so a NaN-boxed pointer held in a plain f64/DOUBLE local
/// never goes stale across a copying GC), pointer-free (the `ts` slot is a
/// raw IEEE double, not a JSValue). Gives `Date` reference semantics so
/// setter mutations propagate through aliasing / function / closure
/// boundaries (#2089).
pub const GC_TYPE_DATE_CELL: u8 = 17;
/// A `Temporal.*` cell (`TemporalCell { kind, value }`) wrapping a `temporal_rs`
/// value (Duration / Instant / PlainDate / …). One shared tag with an internal
/// `TemporalKind` discriminator rather than 9 separate tags (#4687).
/// Arena-allocated, non-movable (a NaN-boxed pointer held in a plain f64/DOUBLE
/// local stays valid across GC), and `pointer_free` from the GC's view — the
/// embedded `temporal_rs` value holds plain integers/`'static` calendar data,
/// never a JSValue. Heap-owning variants (a `ZonedDateTime`'s IANA timezone
/// string) are released by the `TemporalCleanup` finalize hook on sweep.
pub const GC_TYPE_TEMPORAL: u8 = 18;
pub const GC_TYPE_MAX: u8 = GC_TYPE_TEMPORAL;

pub(super) const MALLOC_KIND_UNKNOWN_INDEX: usize = 0;
pub(super) const MALLOC_KIND_BUCKET_COUNT: usize = GC_TYPE_MAX as usize + 1;

pub const LARGE_OBJECT_THRESHOLD_BYTES: usize = 16 * 1024;

#[inline]
pub fn is_large_object_total_size(total_size: usize) -> bool {
    total_size > LARGE_OBJECT_THRESHOLD_BYTES
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcAllocationPolicy {
    Arena,
    Malloc,
    ArenaOrMalloc,
    RawOrLargeOldArena,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcRewriteDescriptorKind {
    Leaf,
    Array,
    Object,
    Closure,
    Promise,
    Error,
    Map,
    LazyArray,
    Set,
    NativeTypedView,
    NativePodView,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcLayoutSlotKind {
    None,
    ArrayElements,
    ObjectFields,
    ClosureCaptures,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcExternalBytePolicy {
    None,
    InlinePayload,
    SideAllocation,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcLargeObjectPolicy {
    OldArenaWhenOverThreshold,
    MallocTracked,
    NotApplicable,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcMoveHookKind {
    None,
    ObjectOverflowFields,
    ClosureDynamicProps,
    MapSideTables,
    SetSideTables,
    /// Rekey a movable exotic cell's address-keyed expando side table after a
    /// move. Used by `GC_TYPE_PROMISE`, whose `status`/`value` expandos
    /// (#5142) live in `object::exotic_expando` keyed by the promise address.
    ExoticExpandoOwner,
    /// Rekey the error side tables (`node_submodules::diagnostics`:
    /// ERROR_MESSAGE_{CODES,SYSCALLS,ERRNOS,PATHS,DESTS,HOSTNAMES} and
    /// ERROR_USER_PROPS — all keyed by the ErrorHeader address) after a
    /// move. Errors are movable; without this a moved error lost its
    /// `err.code`/`err.syscall`/user-assigned props.
    ErrorSideTables,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcRewriteHookKind {
    None,
    SetIndex,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcFinalizeHookKind {
    None,
    MapSideAllocation,
    SetSideAllocation,
    PromiseCleanup,
    NativeArenaOwner,
    NativeTypedView,
    NativeHandle,
    NativePodView,
    /// Drop the embedded `temporal_rs` value in a `GC_TYPE_TEMPORAL` cell so a
    /// heap-owning variant (e.g. a `ZonedDateTime` IANA timezone string) is
    /// released when the cell is swept. POD variants drop to a no-op.
    TemporalCleanup,
    /// Drop a swept error's entries from the address-keyed error side tables
    /// so a fresh error allocated at the recycled address doesn't inherit
    /// the dead error's codes/props.
    ErrorSideTables,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GcTypeInfo {
    pub(crate) type_id: u8,
    pub(crate) name: &'static str,
    pub(crate) allocation_policy: GcAllocationPolicy,
    pub(crate) arena_walkable: bool,
    pub(crate) rewrite_descriptor_kind: GcRewriteDescriptorKind,
    pub(crate) layout_slot_kind: GcLayoutSlotKind,
    pub(crate) movable: bool,
    pub(crate) external_byte_policy: GcExternalBytePolicy,
    pub(crate) large_object_policy: GcLargeObjectPolicy,
    pub(crate) pointer_free: bool,
    pub(crate) move_hook_kind: GcMoveHookKind,
    pub(crate) rewrite_hook_kind: GcRewriteHookKind,
    pub(crate) finalize_hook_kind: GcFinalizeHookKind,
}

pub(super) const fn gc_type_info_entry(
    type_id: u8,
    name: &'static str,
    allocation_policy: GcAllocationPolicy,
    arena_walkable: bool,
    rewrite_descriptor_kind: GcRewriteDescriptorKind,
    layout_slot_kind: GcLayoutSlotKind,
    movable: bool,
    external_byte_policy: GcExternalBytePolicy,
    large_object_policy: GcLargeObjectPolicy,
    pointer_free: bool,
    move_hook_kind: GcMoveHookKind,
    rewrite_hook_kind: GcRewriteHookKind,
    finalize_hook_kind: GcFinalizeHookKind,
) -> GcTypeInfo {
    GcTypeInfo {
        type_id,
        name,
        allocation_policy,
        arena_walkable,
        rewrite_descriptor_kind,
        layout_slot_kind,
        movable,
        external_byte_policy,
        large_object_policy,
        pointer_free,
        move_hook_kind,
        rewrite_hook_kind,
        finalize_hook_kind,
    }
}

pub(super) static GC_TYPE_INFO_BY_ID: [Option<GcTypeInfo>; MALLOC_KIND_BUCKET_COUNT] = [
    None,
    Some(gc_type_info_entry(
        GC_TYPE_ARRAY,
        "array",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Array,
        GcLayoutSlotKind::ArrayElements,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_OBJECT,
        "object",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Object,
        GcLayoutSlotKind::ObjectFields,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::ObjectOverflowFields,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_STRING,
        "string",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_CLOSURE,
        "closure",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Closure,
        GcLayoutSlotKind::ClosureCaptures,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::ClosureDynamicProps,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_PROMISE,
        "promise",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Promise,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
        false,
        // #5142: a promise is movable, but user-attached expando properties
        // (`p.status = …`) live in `object::exotic_expando` keyed by the
        // promise address — rekey that entry when the promise relocates.
        GcMoveHookKind::ExoticExpandoOwner,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::PromiseCleanup,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BIGINT,
        "bigint",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_ERROR,
        "error",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Error,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::ErrorSideTables,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::ErrorSideTables,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_MAP,
        "map",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Map,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::MapSideTables,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::MapSideAllocation,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_LAZY_ARRAY,
        "lazy_array",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::LazyArray,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BUFFER,
        "buffer",
        GcAllocationPolicy::RawOrLargeOldArena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_TYPED_ARRAY,
        "typed_array",
        GcAllocationPolicy::RawOrLargeOldArena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_SET,
        "set",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Set,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::SetSideTables,
        GcRewriteHookKind::SetIndex,
        GcFinalizeHookKind::SetSideAllocation,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_NATIVE_ARENA_OWNER,
        "native_arena_owner",
        GcAllocationPolicy::Malloc,
        false,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::MallocTracked,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::NativeArenaOwner,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_NATIVE_TYPED_VIEW,
        "native_typed_view",
        GcAllocationPolicy::Malloc,
        false,
        GcRewriteDescriptorKind::NativeTypedView,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::NativeTypedView,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_NATIVE_HANDLE,
        "native_handle",
        GcAllocationPolicy::Malloc,
        false,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::NativeHandle,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_NATIVE_POD_VIEW,
        "native_pod_view",
        GcAllocationPolicy::Malloc,
        false,
        GcRewriteDescriptorKind::NativePodView,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::NativePodView,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_DATE_CELL,
        "date",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        // Non-movable: a Date is referenced by a NaN-boxed pointer kept in a
        // plain f64/DOUBLE local that codegen does NOT shadow-root. The
        // conservative stack scan keeps it alive; keeping the address stable
        // means that un-rooted pointer never goes stale across a GC move.
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // pointer_free: the single `ts` slot is a raw f64, never a JSValue.
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_TEMPORAL,
        "temporal",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        // Non-movable: like Date, a Temporal value is referenced by a NaN-boxed
        // pointer kept in a plain f64/DOUBLE local that codegen does NOT
        // shadow-root. The conservative stack scan keeps it alive; a stable
        // address means that un-rooted pointer never goes stale across a GC.
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // pointer_free: the embedded `temporal_rs` value is plain integers +
        // `'static` calendar data, never a JSValue. Any Rust-heap it owns is
        // released by the TemporalCleanup finalize hook, not GC tracing.
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::TemporalCleanup,
    )),
];

#[inline]
pub(crate) fn gc_type_info(obj_type: u8) -> Option<&'static GcTypeInfo> {
    GC_TYPE_INFO_BY_ID
        .get(obj_type as usize)
        .and_then(Option::as_ref)
}

pub(crate) fn gc_type_infos() -> impl Iterator<Item = &'static GcTypeInfo> {
    GC_TYPE_INFO_BY_ID.iter().filter_map(Option::as_ref)
}

#[inline]
pub(crate) fn gc_type_is_arena_walkable(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_some_and(|info| info.arena_walkable)
}

#[inline]
pub(crate) fn gc_type_rewrite_descriptor_kind(obj_type: u8) -> GcRewriteDescriptorKind {
    gc_type_info(obj_type).map_or(GcRewriteDescriptorKind::Leaf, |info| {
        info.rewrite_descriptor_kind
    })
}

#[inline]
pub(crate) fn gc_type_layout_slot_kind(obj_type: u8) -> GcLayoutSlotKind {
    gc_type_info(obj_type).map_or(GcLayoutSlotKind::None, |info| info.layout_slot_kind)
}

#[inline]
pub(crate) fn gc_type_is_movable(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_some_and(|info| info.movable)
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
#[inline]
pub(crate) fn gc_type_external_byte_policy(obj_type: u8) -> GcExternalBytePolicy {
    gc_type_info(obj_type).map_or(GcExternalBytePolicy::None, |info| info.external_byte_policy)
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
#[inline]
pub(crate) fn gc_type_large_object_policy(obj_type: u8) -> GcLargeObjectPolicy {
    gc_type_info(obj_type).map_or(GcLargeObjectPolicy::NotApplicable, |info| {
        info.large_object_policy
    })
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
#[inline]
pub(crate) fn gc_type_is_pointer_free(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_none_or(|info| info.pointer_free)
}

#[inline]
pub(crate) fn gc_type_rewrite_hook_kind(obj_type: u8) -> GcRewriteHookKind {
    gc_type_info(obj_type).map_or(GcRewriteHookKind::None, |info| info.rewrite_hook_kind)
}

pub(crate) fn gc_type_after_payload_move(obj_type: u8, old_user: usize, new_user: usize) {
    match gc_type_info(obj_type).map_or(GcMoveHookKind::None, |info| info.move_hook_kind) {
        GcMoveHookKind::None => {}
        GcMoveHookKind::ObjectOverflowFields => {
            crate::object::overflow_fields_owner_moved(old_user, new_user);
            // #2820: migrate any recorded `Object.setPrototypeOf` entry for
            // this ordinary object so getPrototypeOf/inherited reads still
            // resolve after evacuation.
            crate::object::prototype_chain::object_static_prototype_owner_moved(old_user, new_user);
        }
        GcMoveHookKind::ClosureDynamicProps => {
            crate::closure::closure_dynamic_props_owner_moved(old_user, new_user);
        }
        GcMoveHookKind::MapSideTables => {
            crate::map::map_header_moved_for_gc(old_user, new_user);
        }
        GcMoveHookKind::SetSideTables => {
            crate::set::set_header_moved_for_gc(old_user, new_user);
        }
        GcMoveHookKind::ExoticExpandoOwner => {
            crate::object::exotic_expando::exotic_expando_owner_moved(old_user, new_user);
        }
        GcMoveHookKind::ErrorSideTables => {
            crate::node_submodules::diagnostics_gc::error_side_tables_owner_moved(
                old_user, new_user,
            );
        }
    }
}

pub(crate) fn gc_type_clear_dead_payload_side_tables(obj_type: u8, user_ptr: usize) {
    match gc_type_info(obj_type).map_or(GcMoveHookKind::None, |info| info.move_hook_kind) {
        GcMoveHookKind::ObjectOverflowFields => {
            crate::object::clear_overflow_for_ptr(user_ptr);
        }
        GcMoveHookKind::ClosureDynamicProps => {
            // 2026-07-09 GC audit wave 2: previously an explicit no-op — a
            // dead closure's `fn.prop = …` / `setPrototypeOf(fn, …)` entries
            // leaked and resurrected on a new closure at the reused address.
            crate::closure::clear_closure_side_tables_for_dead_ptr(user_ptr);
        }
        GcMoveHookKind::ErrorSideTables => {
            crate::node_submodules::diagnostics_gc::error_side_tables_clear_dead(user_ptr);
        }
        GcMoveHookKind::None
        | GcMoveHookKind::MapSideTables
        | GcMoveHookKind::SetSideTables
        | GcMoveHookKind::ExoticExpandoOwner => {}
    }
}

pub(crate) unsafe fn gc_type_finalize_unmarked_payload(obj_type: u8, user_ptr: *mut u8) {
    match gc_type_info(obj_type).map_or(GcFinalizeHookKind::None, |info| info.finalize_hook_kind) {
        GcFinalizeHookKind::None => {}
        GcFinalizeHookKind::MapSideAllocation => {
            crate::map::finalize_map_side_allocation_for_gc(user_ptr as *mut crate::map::MapHeader);
        }
        GcFinalizeHookKind::SetSideAllocation => {
            crate::set::finalize_set_side_allocation_for_gc(user_ptr as *mut crate::set::SetHeader);
        }
        GcFinalizeHookKind::PromiseCleanup => {
            let promise = user_ptr as *mut crate::promise::Promise;
            crate::async_hooks::enqueue_gc_destroy((*promise).async_id);
            crate::promise::clear_promise_context_for_gc(promise);
        }
        GcFinalizeHookKind::NativeArenaOwner => {
            crate::native_arena::finalize_native_arena_owner_for_gc(
                user_ptr as *mut crate::native_arena::NativeArenaOwnerHeader,
            );
        }
        GcFinalizeHookKind::NativeTypedView => {
            crate::native_arena::finalize_native_typed_view_for_gc(
                user_ptr as *mut crate::native_arena::NativeTypedViewHeader,
            );
        }
        GcFinalizeHookKind::NativeHandle => {
            crate::native_handle::finalize_native_handle_for_gc(
                user_ptr as *mut crate::native_handle::NativeHandleHeader,
            );
        }
        GcFinalizeHookKind::NativePodView => {
            crate::native_arena::finalize_native_pod_view_for_gc(
                user_ptr as *mut crate::native_arena::NativePodViewHeader,
            );
        }
        GcFinalizeHookKind::TemporalCleanup => {
            crate::temporal::finalize_temporal_cell_for_gc(
                user_ptr as *mut crate::temporal::TemporalCell,
            );
        }
        GcFinalizeHookKind::ErrorSideTables => {
            crate::node_submodules::diagnostics_gc::error_side_tables_clear_dead(user_ptr as usize);
        }
    }
}

#[cfg(feature = "diagnostics")]
#[inline]
pub(super) fn gc_type_name(obj_type: u8) -> &'static str {
    gc_type_info(obj_type).map_or("unknown", |info| info.name)
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
pub(crate) fn validate_gc_type_info(info: &GcTypeInfo) -> Result<(), &'static str> {
    let descriptor_is_leaf = info.rewrite_descriptor_kind == GcRewriteDescriptorKind::Leaf;
    if info.pointer_free {
        if !descriptor_is_leaf {
            return Err("pointer-free GC type exposes a rewrite descriptor");
        }
        if info.layout_slot_kind != GcLayoutSlotKind::None {
            return Err("pointer-free GC type exposes pointer slots");
        }
        return Ok(());
    }

    if descriptor_is_leaf {
        return Err("pointerful GC type lacks rewrite descriptor metadata");
    }

    match info.rewrite_descriptor_kind {
        GcRewriteDescriptorKind::Array => {
            if info.layout_slot_kind != GcLayoutSlotKind::ArrayElements {
                return Err("array rewrite descriptor must expose array element slots");
            }
        }
        GcRewriteDescriptorKind::Object => {
            if info.layout_slot_kind != GcLayoutSlotKind::ObjectFields {
                return Err("object rewrite descriptor must expose object field slots");
            }
        }
        GcRewriteDescriptorKind::Closure => {
            if info.layout_slot_kind != GcLayoutSlotKind::ClosureCaptures {
                return Err("closure rewrite descriptor must expose closure capture slots");
            }
        }
        GcRewriteDescriptorKind::Promise
        | GcRewriteDescriptorKind::Error
        | GcRewriteDescriptorKind::Map
        | GcRewriteDescriptorKind::LazyArray
        | GcRewriteDescriptorKind::Set => {
            if info.layout_slot_kind != GcLayoutSlotKind::None {
                return Err(
                    "external-backed rewrite descriptor must not expose payload layout slots",
                );
            }
        }
        GcRewriteDescriptorKind::NativeTypedView | GcRewriteDescriptorKind::NativePodView => {
            if info.layout_slot_kind != GcLayoutSlotKind::None {
                return Err("native view rewrite descriptor must use fixed slots only");
            }
        }
        GcRewriteDescriptorKind::Leaf => unreachable!("leaf handled above"),
    }

    Ok(())
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
pub(crate) fn validate_gc_type_metadata() -> Result<(), String> {
    for info in gc_type_infos() {
        validate_gc_type_info(info)
            .map_err(|reason| format!("invalid GC metadata for {}: {}", info.name, reason))?;
    }
    Ok(())
}

// Flag constants
pub const GC_FLAG_MARKED: u8 = 0x01;
pub const GC_FLAG_ARENA: u8 = 0x02;
pub const GC_FLAG_PINNED: u8 = 0x04;
/// Set on a keys-array that was handed out by `shape_cache_insert`.
/// `js_object_set_field_by_name` reads this bit to decide whether it
/// must clone before mutating (shared arrays can't be mutated in
/// place; fresh arrays allocated in the `keys.is_null()` branch can).
/// Without the bit the clone fires on every property added to every
/// fresh object literal — a 20-property row object allocates 19
/// throwaway keys_array clones per row.
pub const GC_FLAG_SHAPE_SHARED: u8 = 0x08;
/// Set on strings that live in the intern table. Prevents in-place
/// mutation and allows `js_object_set_field_by_name` to skip the
/// FNV-1a hash (pointer identity is sufficient for interned strings).
pub const GC_FLAG_INTERNED: u8 = 0x10;
/// Gen-GC Phase C4: object has survived at least PROMOTION_AGE
/// minor GCs and is now logically tenured — minor GC trace skips
/// recursion into its fields, exactly like an OLD_ARENA-allocated
/// object. Stored on the GcHeader so the per-object check is one
/// byte load + one bit-and. Non-moving generational: tenured
/// objects stay physically in nursery (no copying / forwarding-
/// pointer machinery), but the trace pretends they're old-gen.
/// True compacting evacuation lands in Phase C4b.
pub const GC_FLAG_TENURED: u8 = 0x20;
/// Gen-GC Phase C4: object has survived at least one minor GC.
/// The non-copying minor path still uses this as its one-bit
/// pre-tenure state; the copied-nursery path stores its exact
/// short age in `_reserved` so loop-carried transients get one
/// extra survivor cycle before old-gen promotion.
pub const GC_FLAG_HAS_SURVIVED: u8 = 0x40;
/// Object's user payload begins with a forwarding address. The new
/// address is stored in the **user-payload's first 8 bytes**
/// (immediately after the GcHeader). Walkers that encounter a
/// FORWARDED header read the forwarding address and follow it;
/// ref-rewrite passes update every NaN-boxed pointer they observe to
/// the forwarded address.
///
/// Two runtime paths use the same bit and payload layout:
/// - GC evacuation/copying stubs are short-lived. Evacuation keeps an
///   explicit list of original nursery headers and clears this bit
///   after owned references have been rewritten/verified, so sweep can
///   reclaim the original slot. Copying nursery stubs disappear when
///   from-space is reset.
/// - Array-growth stubs are intentionally retained. `clean_arr_ptr`
///   follows those chains for stale array references that the runtime
///   cannot rewrite.
///
/// Conservative-stack scans STILL get the old (now-stale) address;
/// objects that might be conservatively referenced are pinned out of
/// the evacuation set via `GC_FLAG_PINNED` to avoid corrupting reads
/// from those words.
///
/// This is the last bit in the u8 gc_flags. Adding more flags
/// requires extending GcHeader (currently 8 bytes total — extending
/// breaks ABI everywhere; deferred until/unless a future phase
/// genuinely needs more bits).
pub const GC_FLAG_FORWARDED: u8 = 0x80;

/// Read the forwarding address embedded in a forwarded object's user
/// payload. Caller must verify `gc_flags & GC_FLAG_FORWARDED` is set;
/// reading otherwise returns garbage. The forwarded address is the
/// **user pointer** of the new location — i.e. what the allocating
/// path returned for the new copy. Callers that need the new GcHeader
/// subtract `GC_HEADER_SIZE` themselves.
///
/// # Safety
/// `header` must point to a valid GcHeader whose user payload is
/// at least 8 bytes (every Perry object's payload is — strings
/// have at least the StringHeader, arrays have ArrayHeader, etc.).
#[inline]
pub unsafe fn forwarding_address(header: *const GcHeader) -> *mut u8 {
    debug_assert!(
        (*header).gc_flags & GC_FLAG_FORWARDED != 0,
        "forwarding_address called on non-forwarded header"
    );
    let user_ptr = (header as *const u8).add(GC_HEADER_SIZE) as *const *mut u8;
    *user_ptr
}

/// Install a forwarding address in an object's user payload and set
/// `GC_FLAG_FORWARDED` on its header. The first 8 bytes of the user
/// payload become the forwarding pointer (the new user address).
/// Subsequent reads via `forwarding_address` recover the new location.
///
/// GC evacuation must later clear this bit only for the originals it
/// just moved. Array growth uses the same representation but leaves the
/// stub retained so stale array references can continue to resolve via
/// `clean_arr_ptr`.
///
/// # Safety
/// As `forwarding_address`. The user payload must be at least 8
/// bytes; this is true for every Perry GC type today.
#[inline]
pub unsafe fn set_forwarding_address(header: *mut GcHeader, new_user_addr: *mut u8) {
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as *mut *mut u8;
    *user_ptr = new_user_addr;
    (*header).gc_flags |= GC_FLAG_FORWARDED;
}

// Object flags stored in GcHeader._reserved (u16) for Object.freeze/seal/preventExtensions
pub const OBJ_FLAG_FROZEN: u16 = 0x01;
pub const OBJ_FLAG_SEALED: u16 = 0x02;
pub const OBJ_FLAG_NO_EXTEND: u16 = 0x04;
// #1175: object was created with a null prototype (Object.create(null) /
// querystring.parse). `Object.getPrototypeOf` returns null for these.
// Bit 6 -- bits 3..5 are the copied-nursery survival counter
// (`GC_COPY_SURVIVAL_AGE_MASK = 0x0038`) and bits 14..15 the layout state,
// so 0x08 would be clobbered on every minor GC. Bits 6..13 are free.
pub const OBJ_FLAG_NULL_PROTO: u16 = 0x40;
// Array carries per-index property descriptors (accessors or custom attrs
// installed via `Object.defineProperty`, or a non-writable `length`). The
// raw-f64 numeric fast paths must decline and route through the
// descriptor-aware element get/set. Bit 10 — bits 7/8/9 are taken by
// `GC_ARRAY_RAW_F64_LAYOUT` (0x80), `OBJ_FLAG_TYPED_ARRAY_PROTO` (0x100),
// and `GC_ARRAY_ARGUMENTS_OBJECT` (0x200). Only meaningful for
// `GC_TYPE_ARRAY`.
pub const OBJ_FLAG_ARRAY_DESCRIPTORS: u16 = 0x400;
// #5054: a property/accessor descriptor (or builtin attrs) has been installed
// on this specific object — the dynamic-write fast path must take the full
// descriptor-aware OrdinarySet walk. Bit 11; only meaningful for
// `GC_TYPE_OBJECT`. Set-only (clearing a descriptor leaves it set; the slow
// path is always correct).
pub const OBJ_FLAG_HAS_DESCRIPTORS: u16 = 0x800;
// #2145: this object is a per-kind `<TypedArrayCtor>.prototype` whose
// `[[Prototype]]` is the shared `%TypedArray%.prototype` intrinsic.
// `Object.getPrototypeOf(Int8Array.prototype)` returns the cached
// `TYPED_ARRAY_INTRINSIC_PROTO_PTR` (a single object shared across all
// 11 typed-array kinds) when this bit is set.
pub const OBJ_FLAG_TYPED_ARRAY_PROTO: u16 = 0x100;
/// Array payload is stored as canonical raw `f64` values, not NaN-boxed
/// `JSValue` slots. This is only meaningful for `GC_TYPE_ARRAY`; object
/// flags share the same `_reserved` word but never inspect this bit.
pub(crate) const GC_ARRAY_RAW_F64_LAYOUT: u16 = 0x80;
/// Array was synthesized for a function's `arguments` binding. This is only
/// meaningful for `GC_TYPE_ARRAY`; it lets `util.types.isArgumentsObject`
/// distinguish Perry's internal `arguments` arrays from user rest arrays.
pub(crate) const GC_ARRAY_ARGUMENTS_OBJECT: u16 = 0x200;
/// #6011: every element slot in `[0, length)` holds either canonical raw-f64
/// number bits or `TAG_HOLE` — the hole-tolerant sibling of
/// `GC_ARRAY_RAW_F64_LAYOUT`. Set when `new Array(n)` hole-initializes a
/// user-facing array (and by the range-loop guard after a hole-seeing verify
/// pass); cleared alongside the dense flag by `clear_array_numeric_layout`,
/// which every non-numeric store path already funnels through. Lets the
/// packed-f64 range-loop guard skip its whole-array verify walk. Bit 12 —
/// bits 12..13 were the last free `_reserved` bits (see the bit map above).
/// Only meaningful for `GC_TYPE_ARRAY`.
pub(crate) const GC_ARRAY_RAW_F64_HOLES: u16 = 0x1000;

pub(super) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(super) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub(super) const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
pub(super) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub(super) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
