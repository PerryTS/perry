//! ABI layout for platform shadow frames. Native targets expose no frame operations.
/// Entries a frame reserves for its header. One [`ShadowEntry`] carries both
/// header words: `value` = the caller's `frame_top`, `meta` = this frame's
/// slot count.
pub const SHADOW_STACK_HEADER_SLOTS: usize = 1;
/// Entries the backing buffer reserves the first time it grows.
pub const SHADOW_STACK_GROW_RESERVE: usize = 1024;

/// Liveness bit, stored in bit 0 of [`ShadowEntry::meta`].
///
/// A bound slot pointer is the address of a compiled `i64`/`double` local
/// slot and is therefore 8-byte aligned, so bit 0 is always free to carry the
/// liveness flag alongside it. [`js_shadow_slot_bind`] refuses to record a
/// pointer that would collide with the tag rather than truncating one.
pub(crate) const SLOT_ACTIVE: usize = 1;
/// Mask recovering the bound compiled-local address from `meta`.
#[cfg(any(test, not(perry_native_stack_maps)))]
pub(crate) const SLOT_PTR_MASK: usize = !SLOT_ACTIVE;

/// One shadow-stack entry.
///
/// `#[repr(C)]` with `value` first is load-bearing: the GC hands the visitor
/// `&mut entry.value` as a `*mut u64` root slot for unbound entries, so the
/// mirrored word must sit at offset 0 and be 8-byte aligned. 16 bytes also
/// makes indexing a shift rather than a multiply.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ShadowEntry {
    /// The heap word the mutator stored.
    ///
    /// Raw bits rather than a typed pointer because slots hold NaN-boxed
    /// JSValue bits (upper 16 bits are the tag, lower 48 the pointer) — the
    /// GC tracer unwraps the NaN-box the same way it already does for closure
    /// captures.
    pub(crate) value: u64,
    /// `bound_slot_address | SLOT_ACTIVE`.
    ///
    /// The address half is the compiled local/global slot this entry mirrors,
    /// or 0 when the entry is unbound. When present, the GC reads and rewrites
    /// the original slot, not the stale mirror copy. The `SLOT_ACTIVE` bit is
    /// the liveness flag: it lets codegen stop reporting a dead local without
    /// mutating the compiled local slot after last use.
    pub(crate) meta: usize,
}

#[cfg(any(test, not(perry_native_stack_maps)))]
impl ShadowEntry {
    pub(crate) const EMPTY: ShadowEntry = ShadowEntry { value: 0, meta: 0 };

    #[inline(always)]
    pub(crate) fn is_active(self) -> bool {
        self.meta & SLOT_ACTIVE != 0
    }

    /// The compiled local slot this entry mirrors, or null when unbound.
    #[inline(always)]
    pub(crate) fn bound_ptr(self) -> *mut u64 {
        (self.meta & SLOT_PTR_MASK) as *mut u64
    }
}

/// Combined shadow-stack state. Holding every field in one TLS slot
/// halves the macOS `tlv_get_addr` calls in every shadow-stack op
/// (push / pop / slot_set / slot_get / scanner) — those ops fired
/// ~3 M+ times per perf-comprehensive run, and TLS access was the
/// single biggest leaf cost in the post-iter-3 profile (20.9 % leaf
/// samples on `tlv_get_addr`). Replacing `RefCell<Vec<u64>>` with
/// `UnsafeCell<ShadowStackState>` also drops the per-op RefCell
/// borrow accounting.
///
/// Safety: shadow-stack ops are only invoked from compiled JS code
/// (runtime-generated, single-threaded for this TLS) and from GC
/// scanner / rewriter passes. The two never overlap — GC is
/// stop-the-world relative to this TLS, and compiled code can't
/// re-enter the runtime through a path that would touch this state
/// while a GC walk is in progress (no allocation occurs inside the
/// scanner/rewriter, and `GC_FLAG_IN_ALLOC` blocks reentrant GC).
///
/// # Why this is `#[repr(C)]` with a hand-rolled buffer instead of a `Vec`
///
/// Generated code addresses these fields **inline** (#7088): a slot store is an
/// address computation and a `stp` against this struct rather than a call into
/// [`js_shadow_slot_set`] / [`js_shadow_slot_bind`]. That requires the field
/// offsets to be a stable, checkable contract, and `Vec`'s layout is explicitly
/// *not* one — `RawVec`'s field order is unspecified and does move. Read out of
/// the shipped `aarch64` archive at the time of writing, the `Vec` form put
/// `cap` at 0, `ptr` at 8 and `len` at 16; nothing promises that stays true, and
/// a silent reorder would have codegen writing GC roots through the wrong word.
///
/// Splitting the three words out explicitly also drops the type's drop glue,
/// which is what made `thread_local!` emit a lazy destructor-registration check
/// (`ldrb`/`cmp`/`b.eq`, plus a `panic_access_error` edge) on the front of every
/// shadow-stack op. The buffer is still freed at thread exit, by
/// [`ShadowBufferGuard`] rather than by `Vec`'s `Drop`.
///
/// The offsets are published as [`SHADOW_STATE_PTR_OFFSET`],
/// [`SHADOW_STATE_LEN_OFFSET`] and [`SHADOW_STATE_FRAME_TOP_OFFSET`], asserted
/// against `offset_of!` below, and asserted equal to codegen's copy by
/// `perry`'s `shadow_layout_contract` test.
#[repr(C)]
pub struct ShadowStackState {
    /// Base of the entry buffer. Null until the first push grows it.
    pub(crate) ptr: *mut ShadowEntry,
    /// Entries in use (header + slots of every live frame, back to back).
    pub(crate) len: usize,
    /// Entries the allocation can hold.
    pub(crate) cap: usize,
    /// Index into the buffer where the current frame's slot 0 lives.
    /// `usize::MAX` when no frame is pushed (initial state + after
    /// the outermost function returns).
    pub(crate) frame_top: usize,
}

/// Byte offset of [`ShadowStackState::ptr`]. Part of the codegen contract.
pub const SHADOW_STATE_PTR_OFFSET: usize = 0;
/// Byte offset of [`ShadowStackState::len`]. Part of the codegen contract.
///
/// One pointer word per preceding field: 8/24 on LP64, 4/12 on ILP32
/// (wasm32, arm64_32). Codegen's copy is the LP64 value; ILP32 codegen is
/// refused until #11378 makes it target-derived.
pub const SHADOW_STATE_LEN_OFFSET: usize = std::mem::size_of::<usize>();
/// Byte offset of [`ShadowStackState::frame_top`]. Part of the codegen
/// contract.
pub const SHADOW_STATE_FRAME_TOP_OFFSET: usize = 3 * std::mem::size_of::<usize>();
/// Size of one [`ShadowEntry`]. Part of the codegen contract: generated code
/// indexes the buffer by shifting, so this must stay a power of two.
pub const SHADOW_ENTRY_SIZE: usize = 16;
/// Byte offset of [`ShadowEntry::meta`] within an entry. Part of the codegen
/// contract.
pub const SHADOW_ENTRY_META_OFFSET: usize = 8;
/// [`SLOT_ACTIVE`] as a public constant, for the codegen contract.
pub const SHADOW_SLOT_ACTIVE_BIT: usize = SLOT_ACTIVE;

const _: () = {
    assert!(std::mem::offset_of!(ShadowStackState, ptr) == SHADOW_STATE_PTR_OFFSET);
    assert!(std::mem::offset_of!(ShadowStackState, len) == SHADOW_STATE_LEN_OFFSET);
    assert!(std::mem::offset_of!(ShadowStackState, frame_top) == SHADOW_STATE_FRAME_TOP_OFFSET);
    assert!(std::mem::size_of::<ShadowEntry>() == SHADOW_ENTRY_SIZE);
    assert!(std::mem::align_of::<ShadowEntry>() == 8);
    assert!(std::mem::offset_of!(ShadowEntry, value) == 0);
    assert!(std::mem::offset_of!(ShadowEntry, meta) == SHADOW_ENTRY_META_OFFSET);
    // Generated code computes `entry = ptr + slot * SHADOW_ENTRY_SIZE` with a
    // shift, and the GC hands out `&mut entry.value` as a `*mut u64` root slot.
    assert!(SHADOW_ENTRY_SIZE.is_power_of_two());
    // `meta`'s bit 0 is the liveness tag, so it must not overlap a slot pointer.
    assert!(SHADOW_SLOT_ACTIVE_BIT == 1);
    // Drop glue on the TLS type is what forces the lazy-registration check the
    // inline sequence exists to avoid; keep it absent.
    assert!(!std::mem::needs_drop::<ShadowStackState>());
};
