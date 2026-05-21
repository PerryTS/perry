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
pub const GC_TYPE_MAX: u8 = GC_TYPE_TYPED_ARRAY;

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
pub(crate) enum GcTraceRewriteKind {
    FieldScanning,
    Leaf,
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
pub(crate) struct GcTypeInfo {
    pub(crate) type_id: u8,
    pub(crate) name: &'static str,
    pub(crate) allocation_policy: GcAllocationPolicy,
    pub(crate) arena_walkable: bool,
    pub(crate) trace_rewrite_kind: GcTraceRewriteKind,
    pub(crate) movable: bool,
    pub(crate) external_byte_policy: GcExternalBytePolicy,
    pub(crate) large_object_policy: GcLargeObjectPolicy,
}

pub(super) const fn gc_type_info_entry(
    type_id: u8,
    name: &'static str,
    allocation_policy: GcAllocationPolicy,
    trace_rewrite_kind: GcTraceRewriteKind,
    movable: bool,
    external_byte_policy: GcExternalBytePolicy,
    large_object_policy: GcLargeObjectPolicy,
) -> GcTypeInfo {
    GcTypeInfo {
        type_id,
        name,
        allocation_policy,
        arena_walkable: true,
        trace_rewrite_kind,
        movable,
        external_byte_policy,
        large_object_policy,
    }
}

pub(super) static GC_TYPE_INFO_BY_ID: [Option<GcTypeInfo>; MALLOC_KIND_BUCKET_COUNT] = [
    None,
    Some(gc_type_info_entry(
        GC_TYPE_ARRAY,
        "array",
        GcAllocationPolicy::Arena,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_OBJECT,
        "object",
        GcAllocationPolicy::ArenaOrMalloc,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_STRING,
        "string",
        GcAllocationPolicy::ArenaOrMalloc,
        GcTraceRewriteKind::Leaf,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_CLOSURE,
        "closure",
        GcAllocationPolicy::ArenaOrMalloc,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::MallocTracked,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_PROMISE,
        "promise",
        GcAllocationPolicy::Malloc,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BIGINT,
        "bigint",
        GcAllocationPolicy::ArenaOrMalloc,
        GcTraceRewriteKind::Leaf,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_ERROR,
        "error",
        GcAllocationPolicy::Malloc,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_MAP,
        "map",
        GcAllocationPolicy::ArenaOrMalloc,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::MallocTracked,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_LAZY_ARRAY,
        "lazy_array",
        GcAllocationPolicy::Arena,
        GcTraceRewriteKind::FieldScanning,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BUFFER,
        "buffer",
        GcAllocationPolicy::RawOrLargeOldArena,
        GcTraceRewriteKind::Leaf,
        false,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_TYPED_ARRAY,
        "typed_array",
        GcAllocationPolicy::RawOrLargeOldArena,
        GcTraceRewriteKind::Leaf,
        false,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
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
pub(crate) fn gc_type_is_movable(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_some_and(|info| info.movable)
}

#[inline]
pub(super) fn gc_type_name(obj_type: u8) -> &'static str {
    gc_type_info(obj_type).map_or("unknown", |info| info.name)
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

pub(super) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(super) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub(super) const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
pub(super) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub(super) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
