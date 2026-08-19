//! Target-pointer-width-dependent struct layout sizes used by inline codegen.
//!
//! Perry's codegen runs on the 64-bit host but may *emit* code for a 32-bit
//! (ILP32) target — currently `arm64_32-apple-watchos` (Apple Watch Series
//! 4–8 / SE). Any inline IR that bakes in a runtime struct's byte size MUST
//! derive it from the *target* triple, not from the host's `size_of`, or the
//! emitted offsets disagree with the target-compiled `perry-runtime` and every
//! field access reads/writes the wrong bytes (the arm64_32 watchOS class of
//! bug). These helpers are the single source of truth for those
//! target-dependent sizes.

/// True when `target_triple` names a 32-bit-pointer (ILP32) target. `arm64_32`
/// (64-bit registers, 32-bit pointers) is the live case for Perry; the other
/// 32-bit families are matched defensively so a future target is sized
/// correctly rather than silently treated as 64-bit.
pub fn target_is_ilp32(target_triple: &str) -> bool {
    target_triple.starts_with("arm64_32")
        || target_triple.starts_with("armv7")
        || target_triple.starts_with("thumbv7")
        || target_triple.starts_with("wasm32")
        || target_triple.starts_with("i686")
        || target_triple.starts_with("i386")
        // x32: 64-bit ISA with 32-bit pointers — the `x86_64` prefix alone
        // would misclassify it as LP64.
        || target_triple.ends_with("gnux32")
}

/// `std::mem::size_of::<perry_runtime::object::ObjectHeader>()` for the target.
///
/// #8047: `ObjectHeader` is two `u32`s (`class_id` @0, `parent_class_id` @4 —
/// the latter carrying the runtime ShapeId after stamping) followed by the
/// #6759 Phase B `meta` pointer. The keys pointer is derived from that ShapeId.
/// LP64 is naturally 16 bytes; ILP32 carries explicit padding before `meta` so
/// the following 8-byte JSValue slots remain aligned. Both are therefore **16**.
///
/// Inline object allocation, header init, and the property inline-cache fast
/// path all use this as the field-region base
/// (`fields = obj + object_header_size_bytes`). It MUST equal the runtime's
/// `size_of::<ObjectHeader>()`, or inline-constructed objects and runtime-FFI
/// field access diverge and every property read/write is corrupt. (The closure
/// header `type_tag` offset has the analogous problem; that one is handled
/// runtime-side via `perry_runtime::closure::CLOSURE_TYPE_TAG_OFFSET` /
/// `offset_of!`.)
///
/// The value stays an 8-BYTE MULTIPLE, which the f64 field region depends on.
pub fn object_header_size_bytes(_target_triple: &str) -> u64 {
    16
}

/// Minimum number of inline field slots `perry-runtime` allocates for EVERY
/// object, mirroring `perry_runtime::object::INLINE_SLOT_FLOOR`.
///
/// perry-codegen deliberately does not depend on perry-runtime (the same reason
/// `PIC_CACHE_WORDS` is duplicated), so the pairing is held by
/// `inline_slot_floor_matches_runtime` here and
/// `inline_slot_floor_matches_codegen` in `perry-runtime/src/object/tests.rs`:
/// change one and both fail.
///
/// Two independent consumers, with OPPOSITE failure modes — which is why they
/// must share one constant rather than two spellings of the same digit:
///
/// - **`lower_call/new_alloc.rs`** sizes the inline-`new` bump allocation as
///   `max(field_count, INLINE_SLOT_FLOOR)` slots. A value SMALLER than the
///   runtime's makes the runtime's bound checks admit slots the emitted
///   allocation never reserved → writes into the neighbouring arena object.
/// - **the runtime's by-index bounds checks** (`object/field_get_set`,
///   `object/field_set_by_name`) gate every slot write on
///   `slot < max(live_inline_slot_count, INLINE_SLOT_FLOOR)`. A codegen value
///   LARGER than the runtime's would under-allocate for those admitted slots.
///
/// So codegen must be exactly equal, not conservatively either way. (Emitted IR
/// no longer materializes this bound itself: #8067 moved the PIC hit path onto
/// an exact ShapeId match, and `expr/property_get/tests.rs`'s
/// `cached_slot_bound_comes_from_the_shape_descriptor_match` asserts it stays
/// off. #8113 then deleted the `field_count` word it used to reload.)
pub const INLINE_SLOT_FLOOR: u64 = 2;

/// `INLINE_SLOT_FLOOR` as the string literal the IR emitters splice in.
pub const INLINE_SLOT_FLOOR_LIT: &str = "2";

/// The GcHeader (8 bytes).
pub(crate) const GC_HEADER_SIZE_BYTES: u64 = 8;
/// One inline field slot (a NaN-boxed f64).
pub(crate) const FIELD_SLOT_SIZE_BYTES: u64 = 8;

/// Total bytes the inline `new` path bump-allocates for a class instance with
/// `field_count` declared fields: GcHeader + ObjectHeader +
/// `max(field_count, INLINE_SLOT_FLOOR)` slots, rounded up to a slot multiple.
///
/// The round-up is retained as a defensive invariant; #8047 makes the header
/// 16 bytes on both pointer widths, so the total is already 8-aligned.
pub(crate) fn inline_alloc_total_size_bytes(target_triple: &str, field_count: u32) -> u64 {
    let alloc_field_count = std::cmp::max(field_count as u64, INLINE_SLOT_FLOOR);
    let payload_size =
        object_header_size_bytes(target_triple) + alloc_field_count * FIELD_SLOT_SIZE_BYTES;
    (GC_HEADER_SIZE_BYTES + payload_size).next_multiple_of(FIELD_SLOT_SIZE_BYTES)
}

/// The packed `GcHeader` word the inline `new` path stores at byte 0 of a
/// freshly bump-allocated class instance (little-endian):
///
/// ```text
///   bits  0..7   = obj_type   (u8)   GC_TYPE_OBJECT
///   bits  8..15  = gc_flags   (u8)   GC_FLAG_ARENA
///   bits 16..31  = _reserved  (u16)  GC_LAYOUT_POINTER_FREE [| GC_OBJ_TYPED_LAYOUT_INTACT]
///   bits 32..63  = size       (u32)  inline_alloc_total_size_bytes
/// ```
///
/// `typed_layout` is the allocation-time bake selected by
/// `lower_call::typed_shape_init`. Pointer-free layouts need no descriptor;
/// pointer-bearing layouts use a module-init ShapeId whose descriptor is
/// registered once for the process, so both can fold their final state into
/// this constant and skip the per-instance layout call.
///
/// #8122: ONE definition, shared by the allocation site
/// (`lower_call/new_alloc.rs`) and the module-level header-image table
/// (`codegen/mod.rs`) that pre-composes `[gc_packed | class_id | ShapeId<<32]`
/// into a per-class global at module init. Both sides must agree byte for
/// byte — a divergence would publish objects whose recorded size or layout
/// state the collector cannot trust — so the arithmetic lives here and the
/// site cross-checks the table's value against its own before using it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlineTypedLayout {
    None,
    PointerFree,
    SideMask,
}

impl InlineTypedLayout {
    #[inline]
    pub(crate) fn is_baked(self) -> bool {
        self != Self::None
    }
}

pub(crate) fn inline_alloc_gc_packed(
    target_triple: &str,
    field_count: u32,
    typed_layout: InlineTypedLayout,
) -> u64 {
    const GC_TYPE_OBJECT: u64 = 2;
    const GC_FLAG_ARENA: u64 = 0x02;
    // PR #1146: pointer-free hint for inline-allocated regular objects. The
    // field-store sites issue per-slot `js_gc_note_slot_layout` so the GC
    // sees real pointer-bearing slots regardless of this initial tag.
    const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;
    const GC_LAYOUT_SIDE_MASK: u64 = 0x8000;
    /// `GC_OBJ_TYPED_LAYOUT_INTACT` — the bit `class_field_inline_guard`
    /// requires before it will read or write a raw-f64 slot directly.
    /// Runtime-side name: `gc::layout::GC_OBJ_TYPED_LAYOUT_INTACT`.
    const GC_OBJ_TYPED_LAYOUT_INTACT: u64 = 0x1000;
    let reserved = match typed_layout {
        InlineTypedLayout::None => GC_LAYOUT_POINTER_FREE,
        InlineTypedLayout::PointerFree => GC_LAYOUT_POINTER_FREE | GC_OBJ_TYPED_LAYOUT_INTACT,
        InlineTypedLayout::SideMask => GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT,
    };
    GC_TYPE_OBJECT
        | (GC_FLAG_ARENA << 8)
        | (reserved << 16)
        | (inline_alloc_total_size_bytes(target_triple, field_count) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paired with `inline_slot_floor_matches_codegen` in
    /// `perry-runtime/src/object/tests.rs` (#7916).
    #[test]
    fn inline_slot_floor_matches_runtime() {
        assert_eq!(
            INLINE_SLOT_FLOOR, 2,
            "perry-runtime's object::INLINE_SLOT_FLOOR is 2; update both sides together"
        );
        assert_eq!(
            INLINE_SLOT_FLOOR_LIT,
            INLINE_SLOT_FLOOR.to_string(),
            "the spliced literal must be the constant"
        );
        // The inline-`new` allocation is `GcHeader + ObjectHeader + 8 * slots`
        // and the bump allocator's offset invariant requires a multiple of 8.
        for triple in ["aarch64-apple-darwin", "arm64_32-apple-watchos"] {
            let total = 8 + object_header_size_bytes(triple) + 8 * INLINE_SLOT_FLOOR;
            assert_eq!(
                total % 8,
                0,
                "{triple}: floor-sized allocation must be 8-aligned"
            );
        }
    }

    #[test]
    fn object_header_size_matches_pointer_width() {
        // #8047 — 64-bit targets: 2×u32 + one pointer = 16.
        assert_eq!(object_header_size_bytes("aarch64-apple-darwin"), 16);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos"), 16);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos-sim"), 16);
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnu"), 16);
        // ILP32 stays 16 through explicit tail padding.
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnux32"), 16);
        assert_eq!(object_header_size_bytes("arm64_32-apple-watchos"), 16);
    }

    /// Two emitters divide the header size by 8 to get a WORD index. #8047
    /// keeps ILP32 at 16 with explicit padding so that remains exact.
    #[test]
    fn object_header_size_is_a_whole_number_of_heap_words() {
        for triple in [
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "arm64_32-apple-watchos",
            "x86_64-unknown-linux-gnux32",
        ] {
            assert_eq!(
                object_header_size_bytes(triple) % 8,
                0,
                "{triple}: header size must be a whole number of 8-byte heap \
                 words — `object_header_size_bytes(..) / 8` is used as a word \
                 index and truncates silently otherwise"
            );
        }
    }

    #[test]
    fn ilp32_classification() {
        assert!(target_is_ilp32("arm64_32-apple-watchos"));
        // The 64-bit watch target must NOT be treated as ILP32.
        assert!(!target_is_ilp32("aarch64-apple-watchos"));
        assert!(!target_is_ilp32("aarch64-apple-darwin"));
        assert!(!target_is_ilp32("x86_64-pc-windows-msvc"));
    }
}
