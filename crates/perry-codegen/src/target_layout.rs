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
/// `ObjectHeader` is four `u32`s (`object_type`, `class_id`, `parent_class_id`,
/// `field_count` = 16 bytes) followed by two pointers (`keys_array`, and the
/// #6759 Phase B `meta` record pointer): 16 bytes → 32 on 64-bit; 8 bytes → 24
/// on ILP32. Inline object allocation, header init, and the property
/// inline-cache fast path all use this as the field-region base
/// (`fields = obj + object_header_size_bytes`). It MUST equal the runtime's
/// `size_of::<ObjectHeader>()`, or inline-constructed objects and runtime-FFI
/// field access diverge and every property read/write is corrupt. (The closure
/// header `type_tag` offset has the analogous problem; that one is handled
/// runtime-side via `perry_runtime::closure::CLOSURE_TYPE_TAG_OFFSET` /
/// `offset_of!`.)
pub fn object_header_size_bytes(target_triple: &str) -> u64 {
    if target_is_ilp32(target_triple) {
        24
    } else {
        32
    }
}

/// Minimum number of inline field slots every object is allocated with, even
/// when it has fewer fields.
///
/// **Corruption-critical, and the invariant is one-sided.** Three populations
/// have to agree: what the ALLOCATORS physically reserve (this constant on the
/// codegen inline-bump path, `perry_runtime::object::INLINE_SLOT_FLOOR` on the
/// runtime path), and what the BOUNDS CHECKS on the inline field region
/// believe (`INLINE_SLOT_FLOOR_LIT` below, and the runtime's
/// `max(field_count, INLINE_SLOT_FLOOR)`). A bounds check that is *smaller*
/// than the reservation only costs a fall-back to the by-name path; a bounds
/// check that is *larger* writes past the allocation. So the runtime constant
/// must never exceed this one, and both must be moved together.
///
/// perry-codegen deliberately does not depend on perry-runtime, so the value is
/// duplicated rather than imported; `inline_slot_floor_matches_runtime` in
/// `perry-runtime/src/object/alloc.rs` pins the pair.
///
/// #7864 lowered it 4 -> 2. It was 8 before #6712. Every byte here is paid by
/// EVERY object in the heap: a two-field record is
/// `8 (GcHeader) + 32 (ObjectHeader) + 8 * floor` bytes, so the floor is 44% of
/// a `{a, b}` at 4 and 29% at 2. The corpus's small-record benchmarks
/// (`churn`, `push_cls`, `retain`, `tree`) allocate exactly two fields.
pub const INLINE_SLOT_FLOOR: u64 = 2;

/// [`INLINE_SLOT_FLOOR`] as the decimal literal emitted into IR bounds checks.
pub const INLINE_SLOT_FLOOR_LIT: &str = "2";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_header_size_matches_pointer_width() {
        // 64-bit targets: 4×u32 + two 8-byte-aligned pointers (keys_array +
        // #6759 meta) = 32.
        assert_eq!(object_header_size_bytes("aarch64-apple-darwin"), 32);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos"), 32);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos-sim"), 32);
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnu"), 32);
        // arm64_32 watchOS (Series 4–8 / SE): 4×u32 + two 4-byte pointers = 24.
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnux32"), 24);
        assert_eq!(object_header_size_bytes("arm64_32-apple-watchos"), 24);
    }

    #[test]
    fn inline_slot_floor_literal_matches_the_constant() {
        // The IR bounds checks are emitted from the string, the inline-bump
        // allocation size from the integer. They are the same number, and the
        // corruption mode is a bounds check LARGER than the reservation.
        assert_eq!(INLINE_SLOT_FLOOR_LIT, INLINE_SLOT_FLOOR.to_string());
    }

    #[test]
    fn inline_slot_floor_matches_the_runtime() {
        // Pinned against the literal on purpose: perry-codegen does not depend
        // on perry-runtime, so this and `inline_slot_floor_matches_codegen` in
        // `perry-runtime/src/object/alloc.rs` hold the pair. Moving one alone
        // fails the other.
        assert_eq!(INLINE_SLOT_FLOOR, 2);
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
