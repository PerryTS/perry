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
}

/// `std::mem::size_of::<perry_runtime::object::ObjectHeader>()` for the target.
///
/// `ObjectHeader` is four `u32`s (`object_type`, `class_id`, `parent_class_id`,
/// `field_count` = 16 bytes) followed by one pointer (`keys_array`): 8 bytes +
/// 8-byte alignment → 24 on 64-bit; 4 bytes → 20 on ILP32. Inline object
/// allocation, header init, and the property inline-cache fast path all use
/// this as the field-region base (`fields = obj + object_header_size_bytes`).
/// It MUST equal the runtime's `size_of::<ObjectHeader>()`, or inline-
/// constructed objects and runtime-FFI field access diverge by 4 bytes and
/// every property read/write is corrupt. (The closure header `type_tag` offset
/// has the analogous problem; that one is handled runtime-side via
/// `perry_runtime::closure::CLOSURE_TYPE_TAG_OFFSET` / `offset_of!`.)
pub fn object_header_size_bytes(target_triple: &str) -> u64 {
    if target_is_ilp32(target_triple) {
        20
    } else {
        24
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_header_size_matches_pointer_width() {
        // 64-bit targets: 4×u32 + 8-byte aligned pointer = 24. Must stay 24 so
        // the shipping arm64 / x86_64 IR is byte-identical to before this fix.
        assert_eq!(object_header_size_bytes("aarch64-apple-darwin"), 24);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos"), 24);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos-sim"), 24);
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnu"), 24);
        // arm64_32 watchOS (Series 4–8 / SE): 4×u32 + 4-byte pointer = 20.
        assert_eq!(object_header_size_bytes("arm64_32-apple-watchos"), 20);
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
