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
/// (64-bit registers, 32-bit pointers) is the live case for Perry.
///
/// Decided by an allowlist of 64-bit architectures rather than a list of
/// 32-bit ones, so a 32-bit family nobody listed (`i586`, `arm-…-gnueabihf`,
/// `mips`, `powerpc`, …) is sized as ILP32 and refused by
/// [`ilp32_codegen_refusal`] instead of silently getting LP64 offsets.
pub fn target_is_ilp32(target_triple: &str) -> bool {
    let triple = target_triple.to_ascii_lowercase();
    let mut parts = triple.split('-');
    let arch = parts.next().unwrap_or_default();
    // 64-bit ISAs with a 32-bit-pointer ABI, spelled in the environment:
    // x86 x32 (`…-gnux32`) and AArch64 ILP32 (`…-gnu_ilp32`).
    if parts.any(|part| part.ends_with("x32") || part.ends_with("ilp32")) {
        return true;
    }
    let lp64 = matches!(
        arch,
        "x86_64"
            | "x86_64h"
            | "amd64"
            | "aarch64"
            | "aarch64_be"
            | "arm64"
            | "arm64e"
            | "s390x"
            | "sparc64"
            | "sparcv9"
            | "wasm64"
    ) || [
        "riscv64",
        "powerpc64",
        "ppc64",
        "mips64",
        "mipsisa64",
        "loongarch64",
    ]
    .iter()
    .any(|family| arch.starts_with(family));
    !lp64
}

/// Why codegen refuses `target_triple`, or `None` when it can emit for it.
///
/// `perry-runtime` compiles for ILP32 targets (#11376), but generated code is
/// not yet ILP32-correct: the inline shadow-stack frame push/slot stores,
/// `MapHeader::used` and `HotTls` reads bake in LP64 byte offsets, and runtime
/// pointers cross the ABI as `i64`. Emitting anyway would miscompile silently
/// (a shadow root written through the wrong word surfaces cycles later as
/// `TypeError: value is not a function`), so the refusal is loud until #11378
/// makes those target-derived. The only ILP32 triple the driver produces is
/// watchOS `arm64_32`, which is opt-in (`PERRY_WATCHOS_ARM64_32`).
pub fn ilp32_codegen_refusal(target_triple: &str) -> Option<String> {
    // The one ILP32 target with a real lowering: wasm32 WASI, behind the
    // off-by-default `target-wasi` feature (runtime ABI adapters in
    // `crate::wasm32`, ILP32 shadow-stack/inline-path layouts).
    if wasm32_lowering(target_triple) {
        return None;
    }
    target_is_ilp32(target_triple).then(|| {
        format!(
            "target `{target_triple}` has 32-bit pointers, and Perry's LLVM backend \
             does not emit ILP32-correct code yet (shadow-stack, Map and HotTls \
             offsets and the runtime pointer ABI are LP64-only); see \
             https://github.com/PerryTS/perry/issues/11378"
        )
    })
}

/// True when this compile targets wasm32 WASI AND the compiler was built with
/// the `target-wasi` feature — the only condition under which any wasm32
/// lowering runs. Always false in a default build.
pub fn wasm32_lowering(target_triple: &str) -> bool {
    cfg!(feature = "target-wasi")
        && target_triple.starts_with("wasm32")
        && target_triple.contains("wasi")
}

/// Exclusive upper bound accepted by the runtime's `is_valid_obj_ptr` for a
/// candidate GC address. Linux-family AArch64 can use the full low 48-bit VA
/// range; every other target keeps the canonical low-half 47-bit ceiling.
/// Inline pointer guards must use this target-derived value before touching a
/// `GcHeader`, matching `perry-runtime/src/value/addr_class.rs`.
pub(crate) fn heap_addr_upper_bound_exclusive(target_triple: &str) -> u64 {
    let triple = target_triple.to_ascii_lowercase();
    let is_aarch64 = triple.starts_with("aarch64") || triple.starts_with("arm64");
    // HarmonyOS triples contain `linux-ohos`, but Rust exposes `target_os =
    // "ohos"`; keep the 47-bit ceiling there. Android uses the full range.
    let is_linux_family = matches!(
        crate::codegen::target_os_of(target_triple),
        Some("linux" | "android")
    );
    if is_aarch64 && is_linux_family {
        0x1_0000_0000_0000
    } else {
        0x8000_0000_0000
    }
}

/// Inclusive lower bound for a candidate GC address after excluding Perry's
/// small-handle band. Mainstream hosted targets accept low virtual addresses,
/// so the handle ceiling (`RECEIVER_HANDLE_FLOOR`) is the effective floor.
/// Other targets use the runtime's conservative 2 TiB floor before any
/// `GcHeader` dereference.
pub(crate) fn heap_addr_lower_bound_inclusive(target_triple: &str) -> u64 {
    let mainstream_os = matches!(
        crate::codegen::target_os_of(target_triple),
        Some(
            "android"
                | "macos"
                | "linux"
                | "windows"
                | "ios"
                | "tvos"
                | "watchos"
                | "visionos"
                | "wasi"
        )
    );
    if mainstream_os {
        crate::runtime_abi::RECEIVER_HANDLE_FLOOR as u64
    } else {
        0x200_0000_0000
    }
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
/// header's field offsets have the analogous problem; those are handled
/// via `perry_abi::CLOSURE_*` /
/// `offset_of!`.)
///
/// The value stays an 8-BYTE MULTIPLE, which the f64 field region depends on.
pub fn object_header_size_bytes(_target_triple: &str) -> u64 {
    16
}

/// `PerryLayoutKey.off` of slot `slot`: the absolute byte offset from the
/// object, which the runtime's slot readers add instead of `header + 8 * slot`.
pub fn layout_key_off(target_triple: &str, slot: u32) -> u64 {
    object_header_size_bytes(target_triple) + u64::from(slot) * 8
}

/// Header plus `slot_count` slots (the size of a layout birth after the `GcHeader`).
pub fn layout_slots_end(target_triple: &str, slot_count: u32) -> u64 {
    layout_key_off(target_triple, slot_count)
}

/// `PerryLayoutDesc.gc_header_word`: the idle `GcHeader` image of a layout
/// birth (`GC_TYPE_OBJECT`, `GC_FLAG_ARENA`, no reserved bits, total size);
/// the runtime ORs the live birth colour in.
pub fn layout_gc_header_word(target_triple: &str, slot_count: u32) -> u64 {
    const GC_FLAG_ARENA: u64 = 0x02;
    let total = 8 + layout_slots_end(target_triple, slot_count);
    u64::from(crate::runtime_abi::GC_TYPE_OBJECT) | (GC_FLAG_ARENA << 8) | (total << 32)
}

/// Byte offset of `ObjectHeader::meta` — the last word of the header — for
/// the target.
///
/// The metadata pointer is the entry point to `ObjectMeta` (the prototype
/// override, the spill buffer, the Array-subclass elements store), and several
/// inline tiers load it. Keeping the derivation in one place also keeps the
/// object-header-size callsite census stable as tiers are added.
pub fn object_meta_slot_offset_bytes(target_triple: &str) -> u64 {
    let pointer_size = if target_is_ilp32(target_triple) { 4 } else { 8 };
    object_header_size_bytes(target_triple) - pointer_size
}

/// Byte offset of `ObjectMeta::spill` (a `u64` on every target: the record is
/// all 8-byte words). **Must equal the runtime's
/// `offset_of!(ObjectMeta, spill)`**, which `object/meta_record.rs`
/// const-asserts as 32; `spill_layout_matches_codegen` in the runtime pins
/// this copy against it.
pub const OBJECT_META_SPILL_OFFSET_BYTES: u64 = 32;

/// `size_of::<perry_runtime::array::ArrayHeader>()` — the `u32` length and
/// capacity words, the named-layout pointer and the element-kind word — on
/// every target: a spill buffer's elements start right
/// after it (a spill buffer is private and never shifted, so it has no front
/// reserve). Pinned by `spill_layout_matches_codegen` in the runtime.
pub const ARRAY_HEADER_SIZE_BYTES: u64 = 24;

/// `offset_of!(ArrayHeader, ek_prefix)`: the dense prefix P of the element-kind
/// word (EK design §2), the u32 a guarded element read compares its index
/// against and an inline length-lowering store clamps. Pinned by
/// `spill_layout_matches_codegen` in the runtime.
pub const ARRAY_EK_PREFIX_OFFSET_BYTES: u64 = 16;

/// `std::mem::size_of::<perry_runtime::closure::ClosureHeader>()` for the
/// target: `{capture_count: u32, shape_id: u32, info, props}` — 24 bytes
/// on LP64 and 16 on ILP32 (`perry_abi::CLOSURE_HEADER_SIZE` is the LP64
/// value the runtime asserts). Trusted exact-arrow bodies use this offset to
/// read compiler-installed raw box capture pointers directly from their
/// immutable capture slots. Keep the target derivation here: using the
/// compiler host's pointer width would make cross-compiled arm64_32 watchOS
/// closures read past the slot.
pub fn closure_header_size_bytes(target_triple: &str) -> u64 {
    if target_is_ilp32(target_triple) {
        16
    } else {
        crate::runtime_abi::CLOSURE_HEADER_SIZE as u64
    }
}

/// Byte offset of `ClosureHeader::info`, the function object's
/// `JsFunctionInfo` pointer (8 on every target: the u32 capture count and u32
/// ShapeId precede it).
pub fn closure_info_offset_bytes(_target_triple: &str) -> u64 {
    crate::runtime_abi::CLOSURE_INFO_OFFSET as u64
}

/// Minimum number of inline field slots `perry-runtime` allocates for EVERY
/// object, mirroring `perry_runtime::object::INLINE_SLOT_FLOOR`.
///
/// perry-codegen deliberately does not depend on perry-runtime, so the
/// pairing is held by
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

/// The packed `GcHeader` word an inline `new` stores. The object
/// `_reserved` layout-state field is zero: GC tracing reads the birth
/// ShapeId's rep, while arrays and closures retain their layout states.
pub(crate) fn inline_alloc_gc_packed(target_triple: &str, field_count: u32) -> u64 {
    const GC_TYPE_OBJECT: u64 = 2;
    const GC_FLAG_ARENA: u64 = 0x02;
    GC_TYPE_OBJECT
        | (GC_FLAG_ARENA << 8)
        | (inline_alloc_total_size_bytes(target_triple, field_count) << 32)
}

/// Slots an inline empty array birth reserves, the runtime's four-slot
/// append reserve (`js_array_alloc(0)`).
pub(crate) const INLINE_EMPTY_ARRAY_SLOTS: u64 = 4;

/// Total bytes of an inline empty array birth: GcHeader + ArrayHeader +
/// the append reserve.
pub(crate) const INLINE_EMPTY_ARRAY_TOTAL_BYTES: u64 = GC_HEADER_SIZE_BYTES
    + ARRAY_HEADER_SIZE_BYTES
    + INLINE_EMPTY_ARRAY_SLOTS * FIELD_SLOT_SIZE_BYTES;

/// The packed `GcHeader` word an inline empty array birth stores: an arena
/// array whose layout state is pointer-free (every slot is a hole). The
/// runtime admits an arena-page address only when the header carries
/// `GC_FLAG_ARENA` (`tracked_gc_header_admitting`), so the word is derived
/// here from the named constants, never hand-typed.
pub(crate) const fn inline_empty_array_gc_packed() -> u64 {
    const GC_TYPE_ARRAY: u64 = 1;
    const GC_FLAG_ARENA: u64 = 0x02;
    const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;
    GC_TYPE_ARRAY
        | (GC_FLAG_ARENA << 8)
        | (GC_LAYOUT_POINTER_FREE << 16)
        | (INLINE_EMPTY_ARRAY_TOTAL_BYTES << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An inline empty array is an arena array with a pointer-free layout
    /// whose size covers the header and the four-slot reserve. A word
    /// without the arena flag is rejected by the runtime's admission, and
    /// every runtime method then sees an empty array (EK B1a regression).
    #[test]
    fn inline_empty_array_word_is_an_arena_pointer_free_array() {
        let word = inline_empty_array_gc_packed();
        assert_eq!(word & 0xff, 1, "obj_type GC_TYPE_ARRAY");
        assert_ne!((word >> 8) & 0x02, 0, "GC_FLAG_ARENA");
        assert_eq!((word >> 16) & 0xffff, 0x4000, "pointer-free layout state");
        assert_eq!(word >> 32, 8 + ARRAY_HEADER_SIZE_BYTES + 4 * 8);
    }

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
    fn closure_header_size_tracks_target_pointer_width() {
        assert_eq!(closure_header_size_bytes("aarch64-apple-darwin"), 24);
        assert_eq!(closure_header_size_bytes("x86_64-unknown-linux-gnu"), 24);
        assert_eq!(closure_header_size_bytes("arm64_32-apple-watchos"), 16);
        assert_eq!(closure_header_size_bytes("wasm32-unknown-unknown"), 16);
    }

    #[test]
    fn ilp32_targets_are_refused_and_lp64_targets_are_not() {
        for triple in [
            "arm64_32-apple-watchos",
            "wasm32-unknown-unknown",
            "i686-unknown-linux-gnu",
            // Not named anywhere: refused because they are not known 64-bit.
            "i586-unknown-linux-gnu",
            "arm-unknown-linux-gnueabihf",
        ] {
            let refusal = ilp32_codegen_refusal(triple).unwrap_or_else(|| {
                panic!("{triple}: ILP32 codegen would miscompile, it must be refused")
            });
            assert!(
                refusal.contains(triple) && refusal.contains("11378"),
                "{refusal}"
            );
        }
        for triple in [
            "aarch64-apple-watchos",
            "arm64-apple-watchos26.0",
            "x86_64-unknown-linux-gnu",
        ] {
            assert_eq!(ilp32_codegen_refusal(triple), None, "{triple}");
        }
        // wasm32 WASI has a real lowering, but only in a `target-wasi` build.
        assert_eq!(
            ilp32_codegen_refusal("wasm32-unknown-wasip2").is_none(),
            cfg!(feature = "target-wasi"),
            "wasm32 WASI is refused exactly when the feature is off"
        );
    }

    #[test]
    fn wasm32_lowering_needs_the_feature_and_a_wasi_triple() {
        assert_eq!(
            wasm32_lowering("wasm32-unknown-wasip2"),
            cfg!(feature = "target-wasi")
        );
        assert_eq!(
            wasm32_lowering("wasm32-wasip1"),
            cfg!(feature = "target-wasi")
        );
        // Browser wasm has its own backend and never reaches this one.
        assert!(!wasm32_lowering("wasm32-unknown-unknown"));
        assert!(!wasm32_lowering("x86_64-unknown-linux-gnu"));
        assert!(!wasm32_lowering("arm64_32-apple-watchos"));
    }

    /// With the feature, a wasm32 WASI module compiles end to end and its
    /// runtime calls come out adapted (the i64-handle call below would be a
    /// link-time signature mismatch without the pass).
    #[cfg(feature = "target-wasi")]
    #[test]
    fn compile_module_lowers_a_wasm32_wasi_module() {
        let module = perry_hir::Module::new("wasi_smoke");
        let opts = crate::CompileOptions {
            target: Some("wasm32-unknown-wasip2".into()),
            emit_ir_only: true,
            is_entry_module: true,
            output_type: "executable".into(),
            disable_constfn_shapes: false,
            ..Default::default()
        };
        let ir = String::from_utf8(crate::compile_module(&module, opts).expect("wasm32 compiles"))
            .expect("IR is UTF-8");
        assert!(
            ir.contains("target triple = \"wasm32-unknown-wasip2\""),
            "{ir}"
        );
        // Every runtime declaration is the runtime's real signature.
        assert!(
            ir.contains("declare ptr @js_string_from_bytes(ptr, i32)"),
            "{ir}"
        );
    }

    /// The refusal must be reached by the real entry point, not just exist.
    #[test]
    fn compile_module_refuses_an_ilp32_target() {
        let module = perry_hir::Module::new("ilp32_refusal");
        let opts = crate::CompileOptions {
            target: Some("arm64_32-apple-watchos".into()),
            emit_ir_only: true,
            ..Default::default()
        };
        let err =
            crate::compile_module(&module, opts).expect_err("an ILP32 target must not be emitted");
        assert!(err.to_string().contains("11378"), "{err}");
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

    #[test]
    fn heap_address_ceiling_matches_runtime_targets() {
        assert_eq!(
            heap_addr_upper_bound_exclusive("aarch64-unknown-linux-gnu"),
            0x1_0000_0000_0000
        );
        assert_eq!(
            heap_addr_upper_bound_exclusive("aarch64-linux-android"),
            0x1_0000_0000_0000
        );
        for triple in [
            "aarch64-apple-darwin",
            "arm64_32-apple-watchos",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-ohos",
            "x86_64-pc-windows-msvc",
        ] {
            assert_eq!(
                heap_addr_upper_bound_exclusive(triple),
                0x8000_0000_0000,
                "{triple}"
            );
        }
    }

    #[test]
    fn heap_address_floor_matches_runtime_targets_and_handle_band() {
        for triple in [
            "aarch64-apple-darwin",
            "aarch64-apple-ios",
            "arm64_32-apple-watchos",
            "x86_64-unknown-linux-gnu",
            "aarch64-linux-android",
            "x86_64-pc-windows-msvc",
            "wasm32-unknown-wasip2",
        ] {
            assert_eq!(
                heap_addr_lower_bound_inclusive(triple),
                crate::runtime_abi::RECEIVER_HANDLE_FLOOR as u64
            );
        }
        for triple in ["aarch64-unknown-linux-ohos", "riscv64gc-unknown-none-elf"] {
            assert_eq!(
                heap_addr_lower_bound_inclusive(triple),
                0x200_0000_0000,
                "{triple}"
            );
        }
    }

    /// The floor must hold for the LLVM triples codegen really emits, not
    /// just rustc spellings: the host default and every `--target` name.
    #[test]
    fn heap_address_floor_covers_emitted_triples() {
        let mut triples = vec![crate::codegen::default_target_triple()];
        for name in crate::codegen::TARGET_NAMES {
            if let Some(triple) = crate::codegen::resolve_target_triple(name) {
                triples.push(triple);
            }
        }
        for triple in triples {
            let expected = match crate::codegen::target_os_of(&triple) {
                Some(
                    "android" | "macos" | "linux" | "windows" | "ios" | "tvos" | "watchos"
                    | "visionos" | "wasi",
                ) => crate::runtime_abi::RECEIVER_HANDLE_FLOOR as u64,
                _ => 0x200_0000_0000,
            };
            assert_eq!(
                heap_addr_lower_bound_inclusive(&triple),
                expected,
                "{triple}"
            );
        }
        for name in ["harmonyos", "harmonyos-simulator"] {
            let triple = crate::codegen::resolve_target_triple(name).expect(name);
            assert_eq!(
                heap_addr_lower_bound_inclusive(&triple),
                0x200_0000_0000,
                "{triple}"
            );
        }
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
        for ilp32 in [
            "arm64_32-apple-watchos",
            "armv7-linux-androideabi",
            "arm-unknown-linux-gnueabihf",
            "thumbv7neon-unknown-linux-gnueabihf",
            "i386-apple-ios",
            "i586-unknown-linux-gnu",
            "i686-pc-windows-msvc",
            "wasm32-wasip2",
            "mips-unknown-linux-gnu",
            "powerpc-unknown-linux-gnu",
            "riscv32imac-unknown-none-elf",
            "x86_64-unknown-linux-gnux32",
            "aarch64-unknown-linux-gnu_ilp32",
        ] {
            assert!(target_is_ilp32(ilp32), "{ilp32} has 32-bit pointers");
        }
        // Every 64-bit spelling Perry's driver produces, plus the other LP64
        // families. The 64-bit watch target must NOT be treated as ILP32.
        for lp64 in [
            "aarch64-apple-watchos",
            "arm64-apple-watchos26.0",
            "aarch64-apple-darwin",
            "arm64-apple-ios17.0",
            "arm64e-apple-darwin",
            "aarch64-linux-android",
            "aarch64-unknown-linux-ohos",
            "x86_64-pc-windows-msvc",
            "x86_64h-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "riscv64gc-unknown-linux-gnu",
            "powerpc64le-unknown-linux-gnu",
            "s390x-unknown-linux-gnu",
            "loongarch64-unknown-linux-gnu",
        ] {
            assert!(!target_is_ilp32(lp64), "{lp64} has 64-bit pointers");
        }
    }
}

/// Appended immutable-template descriptor identity cell; all old fields stay put.
pub fn layout_descriptor_size_bytes(target: &str) -> usize {
    if target_is_ilp32(target) {
        perry_abi::LAYOUT_DESC_SIZE_ILP32
    } else {
        perry_abi::LAYOUT_DESC_SIZE_LP64
    }
}
pub fn layout_descriptor_identity_offset(target: &str) -> usize {
    if target_is_ilp32(target) {
        perry_abi::LAYOUT_DESC_ID_OFFSET_ILP32
    } else {
        perry_abi::LAYOUT_DESC_ID_OFFSET_LP64
    }
}

#[cfg(test)]
mod dense_layout_tests {
    use super::*;
    #[test]
    fn descriptor_and_image_offsets_match_both_pointer_widths() {
        for target in ["x86_64-unknown-linux-gnu", "aarch64-pc-windows-msvc"] {
            assert_eq!(layout_descriptor_size_bytes(target), 80);
            assert_eq!(layout_descriptor_identity_offset(target), 72);
        }
        for target in ["wasm32-wasi", "arm64_32-apple-watchos"] {
            assert_eq!(layout_descriptor_size_bytes(target), 64);
            assert_eq!(layout_descriptor_identity_offset(target), 56);
        }
        use perry_abi::{PerryLayoutImageV1 as Image, PerryStaticLayoutRequestV1 as Request};
        // Header, literal seeds and the directly indexed lazy descriptor span.
        assert_eq!(
            std::mem::size_of::<Image>(),
            32 + 2 * std::mem::size_of::<usize>()
        );
        assert_eq!(std::mem::align_of::<Image>(), std::mem::size_of::<usize>());
        assert_eq!(std::mem::offset_of!(Image, abi_version), 0);
        assert_eq!(std::mem::offset_of!(Image, static_rows), 4);
        assert_eq!(std::mem::offset_of!(Image, family_first), 8);
        assert_eq!(std::mem::offset_of!(Image, family_count), 12);
        assert_eq!(std::mem::offset_of!(Image, seeds), 16);
        assert_eq!(
            std::mem::offset_of!(Image, lazy_first),
            16 + std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::offset_of!(Image, lazy_count),
            20 + std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::offset_of!(Image, lazy_layouts),
            24 + std::mem::size_of::<usize>()
        );
        assert_eq!(std::mem::size_of::<Request>(), 8);
        assert_eq!(std::mem::offset_of!(Request, layout_word), 4);
    }
}

#[cfg(all(test, feature = "llvm-inprocess"))]
#[test]
fn emitted_descriptor_type_has_exact_llvm_target_layout() {
    use inkwell::{context::Context, targets::TargetData};
    for (layout, size, identity) in [("e-p:64:64-i64:64", 128, 72), ("e-p:32:32-i64:64", 112, 56)] {
        let context = Context::create();
        let ir = format!(
            "%Desc = type {}\n@descriptor = global %Desc zeroinitializer\n",
            crate::layout_desc::DESC_TY
        );
        let module = crate::inprocess::parse_ir_text(&context, &ir, "layout_offsets").unwrap();
        let ty = module.get_struct_type("Desc").unwrap();
        let data = TargetData::create(layout);
        assert_eq!(data.get_abi_size(&ty), size);
        assert_eq!(data.offset_of_element(&ty, 5), Some(12));
        assert_eq!(data.offset_of_element(&ty, 15), Some(identity));
        assert_eq!(data.offset_of_element(&ty, 16), Some(identity + 4));
        assert_eq!(data.offset_of_element(&ty, 17), Some(identity + 8));
        let lp64 = size == 128;
        for (element, lp, ilp) in [(
            18,
            perry_abi::LAYOUT_DESC_GC_HEADER_WORD_OFFSET_LP64,
            perry_abi::LAYOUT_DESC_GC_HEADER_WORD_OFFSET_ILP32,
        )] {
            let want = if lp64 { lp } else { ilp };
            assert_eq!(data.offset_of_element(&ty, element), Some(want as u64));
        }
    }
}
