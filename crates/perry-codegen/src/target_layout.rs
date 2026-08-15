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
/// #8113: `ObjectHeader` is two `u32`s (`class_id` @0, `parent_class_id` @4 —
/// the latter carrying the runtime ShapeId after stamping) followed by two
/// pointers (`keys_array`, and the #6759 Phase B `meta` record pointer):
/// 8 bytes of words → **24 on 64-bit**; → **16 on ILP32**. It was 32/24 while
/// the header also carried `object_type` @0 and `field_count` @12; both were
/// derivable (`GcHeader.obj_type` + the ShapeId descriptor's `object_kind`, and
/// the descriptor's `live_inline_slot_count`) and removing either ALONE saved
/// nothing because the struct re-padded.
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
/// Both values stay 8-BYTE MULTIPLES, which the f64 field region after the
/// header depends on: the ILP32 struct is `{u32, u32, *4, *4}` = 16 with align
/// 4, and allocations are 8-aligned, so slot 0 lands 8-aligned and the arm64_32
/// `i64:64` ABI hazard `lower_call/new_alloc.rs` warns about does not arise.
pub fn object_header_size_bytes(target_triple: &str) -> u64 {
    if target_is_ilp32(target_triple) {
        16
    } else {
        24
    }
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
        // #8113 — 64-bit targets: 2×u32 + two 8-byte-aligned pointers
        // (keys_array + #6759 meta) = 24.
        assert_eq!(object_header_size_bytes("aarch64-apple-darwin"), 24);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos"), 24);
        assert_eq!(object_header_size_bytes("aarch64-apple-watchos-sim"), 24);
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnu"), 24);
        // arm64_32 watchOS (Series 4–8 / SE): 2×u32 + two 4-byte pointers = 16.
        assert_eq!(object_header_size_bytes("x86_64-unknown-linux-gnux32"), 16);
        assert_eq!(object_header_size_bytes("arm64_32-apple-watchos"), 16);
    }

    /// #8113: two emitters divide the header size by 8 to get a WORD index
    /// (`expr/proxy_reflect.rs`, `stmt/loops.rs`). That is only sound while the
    /// size is a multiple of 8 on every target — 24/8 and 16/8 are exact, but
    /// #8047's 16/12 pair would make the ILP32 division silently truncate.
    /// Pin the divisibility rather than the quotient.
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
