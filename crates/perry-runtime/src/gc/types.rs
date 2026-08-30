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
/// #6759 Phase B: a per-object [`crate::object::ObjectMeta`] record — the
/// self-describing metadata a shaped `GC_TYPE_OBJECT` reaches through its
/// `ObjectHeader::meta` slot (custom `[[Prototype]]` today; descriptor
/// tables and exotic kind as later tranches migrate). Reachable ONLY via
/// its owner's header slot, so ordinary tracing gives it exactly the
/// owner's lifetime; movable, and holds one traced NaN-box slot.
pub const GC_TYPE_OBJECT_META: u8 = 19;
/// Native `RegExpHeader`. RegExp used to share `GC_TYPE_OBJECT`, forcing every
/// ObjectHeader consumer to inspect unrelated payload words for a magic value.
/// A distinct GC kind is the authoritative, header-external discriminator.
pub const GC_TYPE_REGEXP: u8 = 20;
pub const GC_TYPE_MAX: u8 = GC_TYPE_REGEXP;

pub(super) const MALLOC_KIND_UNKNOWN_INDEX: usize = 0;
pub(super) const MALLOC_KIND_BUCKET_COUNT: usize = GC_TYPE_MAX as usize + 1;

pub const LARGE_OBJECT_THRESHOLD_BYTES: usize = 16 * 1024;

/// The same threshold for an object that can hold POINTERS (`pointer_free ==
/// false`), i.e. every arena type whose payload is traced: arrays, plain
/// objects, closures.
///
/// # Why the two thresholds cannot be the same number
///
/// Crossing the threshold does not merely change where an object is allocated.
/// [`crate::arena::arena_alloc_gc`] births it in the old generation **and
/// stamps `GC_FLAG_TENURED`**, and a minor collection never sweeps old-gen. So
/// the object — and, if it holds pointers, *everything reachable from it* —
/// is immortal until a full mark-sweep runs. The threshold is therefore a
/// trade between two costs:
///
/// * **copy cost**, paid by a young object that survives: one `memcpy`,
///   bounded by the object's own size;
/// * **retention cost**, paid by a born-tenured object that dies: its bytes,
///   held until the next full collection.
///
/// For a `pointer_free` object those two quantities are the *same* quantity,
/// so trading one against the other at 16 KB is a defensible wash. For a
/// pointer-BEARING object the retention is **transitive and unbounded**: the
/// write barrier records old→young edges out of it, and every minor's dirty
/// scan then marks its children live, whether or not anything still refers to
/// the container.
///
/// That is not hypothetical. `gc-handoff/apps/shapes.ts` builds a 2000-element
/// `Node2D[]` per round and drops it. The backing store is
/// `8 + 2048 * 8 + 8 = 16 400` bytes — over the 16 KB line by 16 bytes — so
/// each round's array was born tenured and never reclaimed, and the remembered
/// set re-marked **94 000 then 118 006** slots through arrays no live reference
/// pointed at. Its young-survival ratio read 739‰ and 925‰ while its actual
/// live set was ~3 200 objects, and its two minor collections cost 94 ms of a
/// 139 ms program. Halving the array to 1000 elements — same total work, one
/// step under the line — took survival to 30‰, the remembered-set marks to 0,
/// and the program to 0.07 s.
///
/// 128 KB is V8's `kMaxRegularHeapObjectSize`, which draws exactly this line
/// for exactly this reason. It sits well inside the copier's two structural
/// ceilings — the 1 MB nursery block and `move_young`'s 1 MiB
/// `MAX_YOUNG_MOVE_BYTES` refusal — so a young object admitted by it is always
/// movable, and the worst-case block fragmentation it can cause is 1/8 of one
/// block.
pub const LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES: usize = 128 * 1024;

#[inline]
pub fn is_large_object_total_size(total_size: usize) -> bool {
    total_size > LARGE_OBJECT_THRESHOLD_BYTES
}

/// The birth-generation threshold for `obj_type`, i.e. the one
/// [`crate::arena::arena_alloc_gc`] applies.
///
/// An unknown type gets the conservative (smaller) threshold: the widened one
/// is justified by the retention argument above, which needs the type table to
/// say the payload is traced.
#[inline]
pub fn large_object_threshold_for_type(obj_type: u8) -> usize {
    match gc_type_info(obj_type) {
        Some(info) if !info.pointer_free => LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES,
        _ => LARGE_OBJECT_THRESHOLD_BYTES,
    }
}

/// Does `total_size` bytes of `obj_type` have to be born in the non-moving old
/// generation?
#[inline]
pub fn is_large_object_total_size_for_type(total_size: usize, obj_type: u8) -> bool {
    total_size > large_object_threshold_for_type(obj_type)
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
    RegExp,
    Closure,
    Promise,
    Error,
    Map,
    LazyArray,
    Set,
    NativeTypedView,
    NativePodView,
    /// #6759 Phase B: one traced NaN-box slot (`ObjectMeta::prototype`).
    ObjectMeta,
    /// #6759 phase 1: a cell whose ONLY traced edge is its metadata record.
    /// `DateCell` was `Leaf` (pointer-free) before it gained a `meta` field.
    MetaOnly,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcLayoutSlotKind {
    None,
    ArrayElements,
    ObjectFields,
    RegExpFields,
    ClosureCaptures,
    /// #6812: ObjectMeta records carry two live edges — the custom
    /// `[[Prototype]]` value and the raw spill-buffer pointer. Before the
    /// spill buffer these were enumerated only on the REWRITE path, which
    /// left them invisible to marking (latent for prototypes, which are
    /// normally rooted elsewhere; fatal for the spill buffer, which is
    /// reachable through meta alone).
    ObjectMeta,
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
    /// Rekey RegExp identity/source registries plus its exotic expando owner
    /// entry. `GC_TYPE_REGEXP` is movable, and all three tables use the
    /// payload address as their key.
    RegExpSideTables,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcRewriteHookKind {
    None,
    SetIndex,
    /// Rebuild the Map pointer-key lookup index (`map::MAP_PTR_INDEX`) after
    /// a GC pass rewrote this Map's entry slots: object/bigint keys are
    /// indexed by their pointer bits (identity) or pointee content (bigints),
    /// both of which go stale when the referenced allocation is evacuated.
    /// Mirrors `SetIndex` (#6084).
    MapIndex,
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
    /// Drop a dead typed array's `TYPED_ARRAY_VIEW_META` entry. The table is
    /// keyed by the header address and records the array's materialized backing
    /// ArrayBuffer; leaving the entry behind both keeps that buffer rooted
    /// forever and lets whatever is allocated at the reused address inherit a
    /// backing that is not its own.
    TypedArrayViewMeta,
    /// Drop the embedded `temporal_rs` value in a `GC_TYPE_TEMPORAL` cell so a
    /// heap-owning variant (e.g. a `ZonedDateTime` IANA timezone string) is
    /// released when the cell is swept. POD variants drop to a no-op.
    TemporalCleanup,
    /// Drop a swept error's entries from the address-keyed error side tables
    /// so a fresh error allocated at the recycled address doesn't inherit
    /// the dead error's codes/props.
    ErrorSideTables,
    /// #7539: free a dead lazy JSON array's tape bytes, which
    /// `json_tape_store` owns outside the GC heap.
    LazyArrayTape,
    /// Drop a dead RegExp cell's entries from every payload-address-keyed
    /// registry. Arena reclamation reaches the equivalent cleanup through the
    /// move-hook dead-owner fan-out; malloc-tracked cells use this finalize
    /// hook instead.
    RegExpSideTables,
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
        GcRewriteHookKind::MapIndex,
        GcFinalizeHookKind::MapSideAllocation,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_LAZY_ARRAY,
        "lazy_array",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::LazyArray,
        GcLayoutSlotKind::None,
        // NOT movable. `json_tape_store` keys a lazy array's tape by its
        // header address, and every caller outside `json_tape` holds raw
        // header pointers across allocations. The header is allocated old-gen
        // and born tenured (`json_tape::alloc_lazy_header_bytes`), so nothing
        // relocates it today; saying so here is what keeps old-page defrag
        // from ever doing so. `true` was vacuous before #7539 anyway — the
        // header was multi-megabyte and never left the old generation.
        false,
        // #7539: the tape is a `json_tape_store` side allocation now, not
        // inline payload. Keeping it inline made the header ~2.4 MB on a
        // 10 k-record blob, which `arena_alloc_gc` routed into the old
        // generation with GC_FLAG_TENURED — reclaimable only by a FULL
        // collection, so per-iteration-dead tapes drove `old_gen_bytes`
        // fulls (6 of 9 fulls on the `field_access` fixture).
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::LazyArrayTape,
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
        GcFinalizeHookKind::TypedArrayViewMeta,
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
        GcRewriteDescriptorKind::MetaOnly,
        GcLayoutSlotKind::None,
        // Movable (#6186, 2026-07-09 GC audit). Directly analogous to
        // `GC_TYPE_PROMISE` above: a pointer-free arena object with
        // address-keyed exotic-expando properties, rekeyed on relocation by
        // the `ExoticExpandoOwner` move hook. The `movable` flag only gates
        // OLD-PAGE DEFRAG (`gc_type_is_movable`, oldgen.rs) — the nursery
        // copied-minor already evacuates eden objects regardless of it — so a
        // promoted, long-lived Date could pin its old-gen page from defrag.
        // Safety: old-page defrag runs only inside MOVING collections (the
        // precise pump-boundary safepoint, where the JS stack is unwound so no
        // un-shadow-rooted Date f64 local is live), the SAME collections at
        // which the nursery already evacuates Dates. Any live reference is a
        // traced heap slot the copy pass rewrites; a Date pointer parked in an
        // un-shadow-rooted local only exists mid-frame, never at a safepoint —
        // the identical invariant that already makes movable Promises sound.
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // pointer_free = FALSE since #6759 phase 1. The cell used to be one
        // raw `f64` and nothing else; it now also carries a `meta` edge, so
        // the collector must scan it. `validate_gc_type_info` enforces this
        // pairing — a pointer-free type may not expose a rewrite descriptor —
        // and caught the flag when it was left at `true`.
        false,
        // `d.foo = …` expandos live in `object::exotic_expando` keyed by the
        // Date address; rekey that entry when the cell relocates (mirrors
        // Promise). Without this a moved Date loses its expando properties.
        GcMoveHookKind::ExoticExpandoOwner,
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
        // Movable (#6186, completing the Date change from #6214 — the
        // non-movable rationale above it was disproven there): `movable`
        // only gates OLD-PAGE DEFRAG, which runs solely inside moving
        // collections at stack-unwound safepoints; the nursery copied-minor
        // already relocates Temporal cells. The embedded `temporal_rs` value
        // survives memcpy (its owned allocations live on the Rust heap and
        // move by value); from-space bulk resets skip per-object finalizers,
        // so a moved cell is never double-dropped — `TemporalCleanup` fires
        // once, wherever the cell finally dies.
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // pointer_free: the embedded `temporal_rs` value is plain integers +
        // `'static` calendar data, never a JSValue. Any Rust-heap it owns is
        // released by the TemporalCleanup finalize hook, not GC tracing.
        true,
        // Expando rekey on relocation, mirroring Date: no-op when the cell
        // has no expando entry.
        GcMoveHookKind::ExoticExpandoOwner,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::TemporalCleanup,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_OBJECT_META,
        "object_meta",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::ObjectMeta,
        GcLayoutSlotKind::ObjectMeta,
        // Movable: the owner's `meta` header slot is a raw-pointer child
        // edge (visited in the Object rewrite descriptor), so evacuation
        // rewrites it like any other reference — no address-keyed side
        // state exists for this type at all.
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // Holds the traced `prototype` NaN-box slot.
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_REGEXP,
        "regexp",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::RegExp,
        GcLayoutSlotKind::RegExpFields,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::RegExpSideTables,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::RegExpSideTables,
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

/// Run the post-rewrite hook for `obj_type` after a GC pass changed one or
/// more of the object's reference slots. Shared by every rewrite call site
/// (remembered-set dirty-slot scan, copying field scan, verify/force-evacuate
/// rewrites) so a new hook kind only needs wiring here.
pub(crate) fn run_gc_rewrite_hook(obj_type: u8, user_ptr: usize) {
    match gc_type_rewrite_hook_kind(obj_type) {
        GcRewriteHookKind::None => {}
        GcRewriteHookKind::SetIndex => {
            crate::set::rebuild_set_index_for_gc(user_ptr as *mut crate::set::SetHeader);
        }
        GcRewriteHookKind::MapIndex => {
            crate::map::rebuild_map_ptr_index_for_gc(user_ptr as *mut crate::map::MapHeader);
        }
    }
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
            crate::object::module_wrapper_owner_moved(old_user, new_user);
        }
        GcMoveHookKind::ClosureDynamicProps => {
            crate::closure::closure_dynamic_props_owner_moved(old_user, new_user);
            crate::closure::closure_box_captures_owner_moved(old_user, new_user);
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
        GcMoveHookKind::RegExpSideTables => {
            crate::regex::regex_header_moved_for_gc(old_user, new_user);
        }
    }
}

pub(crate) fn gc_type_clear_dead_payload_side_tables(obj_type: u8, user_ptr: usize) {
    match gc_type_info(obj_type).map_or(GcMoveHookKind::None, |info| info.move_hook_kind) {
        GcMoveHookKind::ObjectOverflowFields => {
            crate::object::clear_overflow_for_ptr(user_ptr);
            crate::object::clear_module_wrapper_for_dead_ptr(user_ptr);
            // The old per-object KEYS_INDEX prune is gone (#6759 C1): key
            // indexes are shape records keyed on keys_array identity now,
            // memory-pruned by `shapes::prune_dead_shape_keys` in the
            // dead-owner fan-out and correctness-guarded by per-hit
            // content validation.
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
        GcMoveHookKind::RegExpSideTables => {
            crate::regex::regex_header_clear_dead_for_gc(user_ptr);
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
        GcFinalizeHookKind::TypedArrayViewMeta => {
            crate::typedarray_view::clear_view_meta(user_ptr as usize);
        }
        GcFinalizeHookKind::LazyArrayTape => {
            crate::json_tape_store::release(user_ptr as usize);
        }
        GcFinalizeHookKind::RegExpSideTables => {
            crate::regex::regex_header_clear_dead_for_gc(user_ptr as usize);
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
        GcRewriteDescriptorKind::RegExp => {
            if info.layout_slot_kind != GcLayoutSlotKind::RegExpFields {
                return Err("regexp rewrite descriptor must expose regexp fields");
            }
        }
        GcRewriteDescriptorKind::Closure => {
            if info.layout_slot_kind != GcLayoutSlotKind::ClosureCaptures {
                return Err("closure rewrite descriptor must expose closure capture slots");
            }
        }
        GcRewriteDescriptorKind::MetaOnly
        | GcRewriteDescriptorKind::Promise
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
        GcRewriteDescriptorKind::ObjectMeta => {
            // Meta records expose every child edge (prototype, spill buffer,
            // and private-evaluation brand) to marking and rewriting.
            if info.layout_slot_kind != GcLayoutSlotKind::ObjectMeta {
                return Err("object-meta descriptor must expose its child edges to marking");
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
/// #8690: this `GC_TYPE_OBJECT` carries a cached proof that the packed
/// Array-subclass element prefix recorded in `ObjectMeta::flags` is numeric.
/// The bit is the address-reuse-safe authority: fresh allocations start with
/// it clear, and the whole `_reserved` word rides copying/compacting GC moves.
/// Every ordinary object-slot store clears it through `layout_note_slot`; the
/// object-owned spill store has the matching owner-side hook.
///
/// Bit 7 is shared with `GC_ARRAY_RAW_F64_LAYOUT`, which is only meaningful
/// for `GC_TYPE_ARRAY`. The two facts deliberately mean the same thing to the
/// loop guard — direct loads over the admitted prefix are raw numeric f64s —
/// but their payload layouts and invalidation funnels remain type-specific.
pub(crate) const OBJ_FLAG_PACKED_NUMERIC_PROOF: u16 = 0x80;
// Array carries per-index property descriptors (accessors or custom attrs
// installed via `Object.defineProperty`, or a non-writable `length`). The
// raw-f64 numeric fast paths must decline and route through the
// descriptor-aware element get/set. Bit 10 — bits 7/8/9 are taken by
// `GC_ARRAY_RAW_F64_LAYOUT` (0x80), `OBJ_FLAG_TYPED_ARRAY_PROTO` (0x100),
// and `GC_ARRAY_ARGUMENTS_OBJECT` (0x200). Only meaningful for
// `GC_TYPE_ARRAY`.
pub const OBJ_FLAG_ARRAY_DESCRIPTORS: u16 = 0x400;
/// #9064: this `GC_TYPE_OBJECT` has O(1)-delete tombstones whose ShapeId is
/// deliberately stable. A cached slot is authoritative only after its value
/// is checked against `TAG_HOLE`; the marker means the cached key was deleted
/// and the access must take the ordinary lookup path.
///
/// Bit 10 is shared with `OBJ_FLAG_ARRAY_DESCRIPTORS`, which is meaningful
/// only for `GC_TYPE_ARRAY`. Object and Array payloads are disjoint by the
/// already-required `obj_type` guard, matching the existing bit sharing at
/// 7/9/11/12.
pub const OBJ_FLAG_STABLE_TOMBSTONES: u16 = 0x400;
// #5054: a property/accessor descriptor (or builtin attrs) has been installed
// on this specific object — the dynamic-write fast path must take the full
// descriptor-aware OrdinarySet walk. Bit 11; only meaningful for
// `GC_TYPE_OBJECT`. Set-only (clearing a descriptor leaves it set; the slow
// path is always correct). #7480 reuses bit 11 for `GC_TYPE_ARRAY` as
// `GC_ARRAY_ELEMENT_SHAPE`; the two are disjoint by `obj_type`.
pub const OBJ_FLAG_HAS_DESCRIPTORS: u16 = 0x800;
/// Heap class-expression value (`class C {}`), as distinct from an ordinary
/// instance carrying the same `GC_TYPE_OBJECT` allocation tag. This is the
/// authoritative replacement for `ObjectHeader::object_type ==
/// OBJECT_TYPE_CLASS`; #8113 deleted that legacy payload word — the note below
/// is history, kept because it explains why the kind lives in the descriptor
/// rather than in
/// #8047 removes it. Bit 13 is preserved by survival-age and layout-state
/// updates and is otherwise unused for `GC_TYPE_OBJECT`.
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
/// #8098: this `GC_TYPE_OBJECT` allocation is an ORDINARY plain object. It has
/// no class, but it also carries none of the per-object `[[Set]]` semantics a
/// class-less receiver may otherwise have — a `URL`'s `pathname`/`search`/…
/// slots are live views whose setters rebuild `href`, `Object.prototype` is the
/// realm intrinsic, and native-module receivers dispatch. Only a runtime birth
/// site that has established the receiver is ordinary may set this; it is what
/// admits `JSON.parse` output to the object-write fast paths, whose generated
/// hit paths re-test this exact bit on every store, so a ShapeId shared with an
/// unmarked population can never carry one population's cached slot into
/// another's.
///
/// Bit 9 — only meaningful for `GC_TYPE_OBJECT`, disjoint from the array-only
/// `GC_ARRAY_ARGUMENTS_OBJECT` by `obj_type` (its sole reader goes through
/// `array::header::array_gc_header`, which refuses any header that is not
/// `GC_TYPE_ARRAY`), the same sharing bits 11 and 12 already use. The value
/// MUST match `PLAIN_ORDINARY_OBJ_FLAG` in
/// `perry-codegen/src/expr/proxy_reflect.rs`, which emits it as a literal.
pub const OBJ_FLAG_PLAIN_ORDINARY: u16 = 0x200;
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
/// #7480: every element slot in `[0, length)` holds a `POINTER_TAG`-boxed
/// `GC_TYPE_OBJECT` whose `class_id` is the one recorded in the array's
/// element-shape record — the *pointer* sibling of the two raw-f64 bits
/// above, and the shared prerequisite of both element-shape routes.
///
/// This is the O(1) half of the proof and it exists for the same reason the
/// raw-f64 bit does: it rides in `_reserved`, which the copying collector
/// copies verbatim, so the invariant survives a move without a side-table
/// walk. The *shape id* itself cannot fit here, so it lives in the
/// address-keyed record `array::element_shape` maintains, moved on
/// relocation by `layout_transfer` exactly like `TYPED_LAYOUTS`. The bit is
/// the authority: a fresh allocation's `_reserved` is zero, so a stale
/// record left behind at a recycled address is never consulted.
///
/// Bit 11 — shared with `OBJ_FLAG_HAS_DESCRIPTORS`, which is only
/// meaningful for `GC_TYPE_OBJECT`, exactly as `GC_ARRAY_RAW_F64_HOLES`
/// (bit 12) shares with `GC_OBJ_TYPED_LAYOUT_INTACT`. Only meaningful for
/// `GC_TYPE_ARRAY`; every accessor goes through `array::element_shape`,
/// which checks `obj_type` first.
pub(crate) const GC_ARRAY_ELEMENT_SHAPE: u16 = 0x800;

pub(super) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(super) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub(super) const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
pub(super) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub(super) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
