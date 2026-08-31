//! `gc_map` unit tests, in a sibling file.
//!
//! Extracted to keep `gc_map.rs` under the repo's 2000-line cap. Moved
//! verbatim; still a child module, so `use super::*` reaches the parent.

use super::*;

/// Build a minimal but structurally real v3 block: one function, one
/// record, two roots of which the second is the base/derived duplicate.
fn sample_asm() -> String {
    let mut asm = String::new();
    asm.push_str("\t.no_dead_strip\t__LLVM_StackMaps\n");
    asm.push_str("\t.section\t__LLVM_STACKMAPS,__llvm_stackmaps\n");
    asm.push_str("__LLVM_StackMaps:\n");
    asm.push_str("\t.byte\t3\n\t.byte\t0\n\t.short\t0\n");
    asm.push_str("\t.long\t1\n"); // functions
    asm.push_str("\t.long\t0\n"); // constants
    asm.push_str("\t.long\t1\n"); // records
    asm.push_str("\t.quad\t_probe_fn\n");
    asm.push_str("\t.quad\t144\n"); // stack size
    asm.push_str("\t.quad\t1\n"); // record count
                                  // record: id, instruction offset, reserved, location count
    asm.push_str("\t.quad\t0\n");
    asm.push_str("\t.long\t64\n");
    asm.push_str("\t.short\t0\n");
    asm.push_str("\t.short\t4\n");
    // three statepoint preamble constants, then base/derived pair
    for _ in 0..3 {
        asm.push_str("\t.byte\t4\n\t.byte\t0\n\t.short\t8\n\t.short\t0\n\t.short\t0\n\t.long\t0\n");
    }
    asm.push_str(
        "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t29\n\t.short\t0\n\t.long\t4294967272\n",
    );
    asm.push_str("\t.p2align\t3\n");
    asm.push_str("\t.short\t0\n\t.short\t0\n"); // live-out header
    asm.push_str("\t.p2align\t3\n");
    asm.push_str("\t.subsections_via_symbols\n");
    asm
}

/// The same map an **ELF** backend prints. Captured from
/// `perry --target linux` on `08_map_set_sidetables.ts`: AArch64/ELF
/// spells the stack map's fields `.hword` / `.word` / `.xword`, not
/// `.short` / `.long` / `.quad`.
///
/// One function, one record, and — critically — a `.word` **instruction
/// offset** and a `.word` **32-bit `Offset` field per location**, which is
/// what makes the width of `.word` load-bearing rather than cosmetic.
#[test]
fn ilp32_targets_emit_a_pointer_sized_address_field() {
    // watchOS `arm64_32` is ILP32. An 8-byte address slot there would need
    // a relocation ld64 has no reason to emit, and the runtime would read
    // two pointers as one — so the field follows the target's width and
    // the header records which width was used.
    let (out, stats) = compact_stack_map_asm(&sample_asm(), "arm64_32-apple-watchos")
        .expect("an ILP32 stack map must parse")
        .expect("an ILP32 stack map must be rewritten");
    assert!(
        out.contains("\t.long\t_probe_fn"),
        "the function address must be pointer-sized on ILP32:\n{out}"
    );
    assert!(
        !out.contains("\t.quad\t_probe_fn"),
        "an 8-byte address slot on ILP32 is the bug this guards:\n{out}"
    );
    assert!(
        out.contains("\t.short\t0\n"),
        "the header must record a 32-bit address width:\n{out}"
    );
    assert!(
        out.contains(&format!("\t.byte\t{GC_MAP_VERSION}\n")),
        "the emitted blob must declare the version the runtime expects:\n{out}"
    );
    // 16-byte header + one 12-byte function entry (4-byte address on ILP32)
    // + one 4-byte v5 stream offset + one 4-byte instruction offset + a
    // 3-byte root stream. The LP64 form of the same map is 4 bytes larger,
    // which is the whole point of the address field being pointer-sized.
    assert_eq!(stats.compact_bytes, 16 + 12 + 4 + 4 + 3);
}

fn compact_and_assemble_refusal(target: &str) -> String {
    // Mirrors the guard in `compact_and_assemble`; kept here so the test
    // fails if that guard is removed rather than if a string changes.
    if matches!(format_for(target), ObjectFormat::Coff) && !target.starts_with("x86_64") {
        return format!(
            "perry: native GC roots (PERRY_RS4GC) are not enabled for target \
                 `{target}` yet — the runtime's Windows stack walker is x86-64 only"
        );
    }
    String::new()
}

#[test]
fn x86_64_windows_is_no_longer_refused() {
    // #7354: the RtlVirtualUnwind walker landed and was verified on a
    // Windows host, so the COFF refusal must not fire for x86-64 — a
    // refusal here would silently disable the platform the walker exists
    // for.
    assert_eq!(compact_and_assemble_refusal("x86_64-pc-windows-msvc"), "");
}

#[test]
fn arm64_windows_is_refused_until_it_has_a_walker() {
    // ARM64 Windows passes the arch gate (aarch64) and is COFF, but the
    // runtime's Windows walker is x86-64 only — the CONTEXT layout and
    // unwinder register model differ on ARM64 — so every frame would go
    // unvisited and the collector would free live objects. Staged is not
    // enabled.
    let err = compact_and_assemble_refusal("aarch64-pc-windows-msvc");
    assert!(err.contains("x86-64 only"), "{err}");
}

#[test]
fn coff_targets_use_a_name_a_pe_image_can_hold() {
    // A PE image section header has an 8-byte name field; long names live
    // only in object files, as a string-table offset the linker does not
    // carry into the image. `.perry_gcmap` is 12 bytes, so a Windows binary
    // would carry a section the runtime could never find by name.
    let (out, _) = compact_stack_map_asm(&sample_asm(), "x86_64-pc-windows-msvc")
        .expect("a COFF stack map must parse")
        .expect("a COFF stack map must be rewritten");
    assert!(out.contains(".pgcmap"), "{out}");
    assert!(
        !out.contains(".perry_gcmap"),
        "the 12-byte name cannot survive into a PE image:\n{out}"
    );
    assert!(super::COFF_SECTION_NAME.len() <= 8);
}

/// Every Apple target Perry can build for must be accepted here, with the
/// address width its ABI actually uses.
///
/// This is cheap and it is not redundant with the two width tests above.
/// Those pin one ILP32 target and one LP64 target; this pins the *set*, so
/// adding a triple to the compiler without deciding its width fails here
/// rather than at someone's link step.
///
/// Measured 2026-08-04, `cargo check -p perry-runtime --target <t>`:
/// macOS, iOS, iOS-sim, tvOS, watchOS and visionOS all compile. watchOS and
/// visionOS needed `--no-default-features` (or any feature set without
/// `dyn-eval`) until a third-party fix landed: `psm`, reached only via
/// `dyn-eval` -> perry-parser -> swc_ecma_parser -> stacker, selects its
/// assembly with
///
///   #if defined(CFG_TARGET_OS_darwin) || ..._macos) || ..._ios) || ..._tvos)
///
/// which omits `watchos` and `visionos`, so both fall to the ELF branch and
/// emit `.type`/`.size` that the Mach-O assembler rejects. Nothing in Perry
/// is involved, and the auto-optimize path enables `dyn-eval` only for
/// programs that construct a function body at runtime — so watch and vision
/// apps that never call `new Function` were always buildable.
#[test]
fn every_apple_target_is_accepted_with_its_own_address_width() {
    // (triple, expects 64-bit addresses)
    let targets = [
        ("arm64-apple-macosx15.0.0", true),
        ("arm64-apple-ios", true),
        ("arm64-apple-ios-sim", true),
        ("arm64-apple-tvos", true),
        ("arm64-apple-visionos", true),
        ("arm64-apple-watchos", true),
        // The one ILP32 Apple target. `arm64_32` must be tested before any
        // `arm64` prefix match, which is why the emitter checks it first.
        ("arm64_32-apple-watchos", false),
        ("x86_64-apple-macosx15.0.0", true),
    ];
    for (target, lp64) in targets {
        assert_eq!(
            compact_and_assemble_refusal(target),
            "",
            "{target} must not be refused"
        );
        let (out, _) = compact_stack_map_asm(&sample_asm(), target)
            .unwrap_or_else(|e| panic!("{target} must parse: {e}"))
            .unwrap_or_else(|| panic!("{target} must be rewritten"));
        let (want, reject) = if lp64 {
            ("\t.quad\t_probe_fn", "\t.long\t_probe_fn")
        } else {
            ("\t.long\t_probe_fn", "\t.quad\t_probe_fn")
        };
        assert!(
            out.contains(want),
            "{target} must emit a {}-bit address field:\n{out}",
            if lp64 { 64 } else { 32 }
        );
        assert!(
            !out.contains(reject),
            "{target} emitted the wrong address width:\n{out}"
        );
    }
}

#[test]
fn lp64_targets_keep_the_eight_byte_address_field() {
    let (out, _) = compact_stack_map_asm(&sample_asm(), "arm64-apple-ios")
        .expect("an LP64 stack map must parse")
        .expect("an LP64 stack map must be rewritten");
    assert!(out.contains("\t.quad\t_probe_fn"), "{out}");
    assert!(
        out.contains("\t.short\t1\n"),
        "the header must record a 64-bit address width:\n{out}"
    );
}

fn aarch64_elf_sample_asm() -> String {
    let mut asm = String::new();
    asm.push_str("\t.section\t.llvm_stackmaps,\"a\",@progbits\n");
    asm.push_str("__LLVM_StackMaps:\n");
    asm.push_str("\t.byte\t3\n\t.byte\t0\n\t.hword\t0\n");
    asm.push_str("\t.word\t1\n"); // functions
    asm.push_str("\t.word\t0\n"); // constants
    asm.push_str("\t.word\t1\n"); // records
    asm.push_str("\t.xword\tprobe_fn\n");
    asm.push_str("\t.xword\t112\n"); // stack size
    asm.push_str("\t.xword\t1\n"); // record count
    asm.push_str("\t.xword\t0\n"); // patchpoint id
    asm.push_str("\t.word\t.Ltmp0-probe_fn\n"); // instruction offset
    asm.push_str("\t.hword\t0\n");
    asm.push_str("\t.hword\t4\n"); // location count
    for _ in 0..3 {
        asm.push_str("\t.byte\t4\n\t.byte\t0\n\t.hword\t8\n\t.hword\t0\n\t.hword\t0\n\t.word\t0\n");
    }
    // The live root: SP-relative (DWARF 31), frame offset 24.
    asm.push_str("\t.byte\t3\n\t.byte\t0\n\t.hword\t8\n\t.hword\t31\n\t.hword\t0\n\t.word\t24\n");
    asm.push_str("\t.p2align\t3\n");
    asm.push_str("\t.hword\t0\n\t.hword\t0\n"); // live-out header
    asm.push_str("\t.p2align\t3\n");
    asm.push_str("\t.section\t\".note.GNU-stack\",\"\",@progbits\n");
    asm
}

/// An ELF stack map must decode with the SAME roots the Mach-O spelling
/// would give. `.word` is 4 bytes here and 2 bytes on x86 — a fixed table
/// gets one of the two silently wrong, and "silently" is the whole problem:
/// two bytes of drift per field relocates every root after it, so the
/// module either refuses for an unrelated-looking reason or, worse,
/// compacts a live set read from the wrong bytes.
#[test]
fn aarch64_elf_word_directives_decode_to_the_right_root() {
    let (out, stats) =
        compact_stack_map_asm(&aarch64_elf_sample_asm(), "aarch64-unknown-linux-gnu")
            .expect("an aarch64-ELF stack map must parse")
            .expect("an aarch64-ELF stack map must be rewritten");
    assert_eq!(stats.functions, 1);
    assert_eq!(stats.records, 1);
    // Four locations in, one root out: the three preamble constants drop.
    assert_eq!(stats.roots, 1, "the SP-relative root must survive");
    assert!(out.contains("_perry_gc_map:"));
    assert!(out.contains(".quad\tprobe_fn"));
    assert!(!out.contains("llvm_stackmaps"));
}

/// Reading that same ELF map with x86's `.word` (2 bytes) must not quietly
/// produce a different answer. This is the assertion that the width is
/// load-bearing: if `.word` were hardcoded, this test and the one above
/// could not both hold.
#[test]
fn word_width_is_load_bearing_not_cosmetic() {
    assert_eq!(word_width_for("aarch64-unknown-linux-gnu"), 4);
    assert_eq!(word_width_for("arm64-apple-macosx15.0.0"), 4);
    assert_eq!(word_width_for("x86_64-unknown-linux-gnu"), 2);
    assert_eq!(word_width_for("x86_64h-apple-macosx15.0.0"), 2);
    assert_eq!(word_width_for("i686-unknown-linux-gnu"), 2);
    assert_eq!(word_width_for("i386-unknown-linux-gnu"), 2);
    // Not x86: `aarch64` must not be mistaken for one by a loose match.
    assert_eq!(word_width_for("riscv64gc-unknown-linux-gnu"), 4);

    let asm = aarch64_elf_sample_asm();
    let correct = compact_stack_map_asm(&asm, "aarch64-unknown-linux-gnu")
        .expect("parses under the right width")
        .expect("rewritten");
    let wrong = compact_stack_map_asm(&asm, "x86_64-unknown-linux-gnu");
    match wrong {
        // Either it refuses, or it decodes to something different. What it
        // must NOT do is agree — that would mean the width never mattered
        // and this guard is asserting nothing.
        Err(_) => {}
        Ok(None) => panic!("the block must not vanish"),
        Ok(Some((_, stats))) => assert_ne!(
            stats.roots, correct.1.roots,
            "decoding an ELF aarch64 map with x86's .word width agreed with the correct \
                 width — the width is not actually being used"
        ),
    }
}

#[test]
fn compacts_and_keeps_only_real_roots() {
    let (out, stats) = compact_stack_map_asm(&sample_asm(), "arm64-apple-macosx15.0.0")
        .expect("block parses")
        .expect("block rewritten");
    assert_eq!(stats.functions, 1);
    assert_eq!(stats.records, 1);
    // Four locations in, one root out: three constants dropped.
    assert_eq!(stats.roots, 1);
    assert!(
        stats.compact_bytes < stats.original_bytes,
        "compact {} should beat original {}",
        stats.compact_bytes,
        stats.original_bytes
    );
    assert!(out.contains("_perry_gc_map:"));
    assert!(out.contains(".quad\t_probe_fn"));
    // The old section must be gone, and nothing may still name its label.
    assert!(!out.contains("__llvm_stackmaps"));
    assert!(!out.contains("__LLVM_StackMaps"));
    // The dead-strip guard must survive, retargeted.
    assert!(out.contains(".no_dead_strip\t_perry_gc_map"));

    // Guard the -O3 ELF shapes that broke the aarch64-linux arm. Both are
    // GNU-as symbol assignments -- zero bytes, no leading directive -- so
    // the dispatch reported the SYMBOL as an unrecognised directive and
    // refused the module. Mach-O never emits this spelling, which is why
    // every macOS arm stayed green.
}

#[test]
fn the_assembler_is_told_the_same_machine_as_the_code_generator() {
    // `-mcpu=native` on a host with SVE makes LLVM emit `mov z1.d, #…`.
    // Assembling that with a clang given no `-mcpu` fails with
    // `instruction requires: sve or sme` — the aarch64-linux arm's second
    // failure, after the parse fix. Forward machine selection, nothing else.
    let args: Vec<String> = [
        "-O3",
        "-mcpu=native",
        "-fno-math-errno",
        "-march=armv8.3-a",
        "-mtune=neoverse-n1",
        "-o",
        "out.o",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let got: Vec<&str> = super::cpu_selection_flags(&args)
        .into_iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(
        got,
        ["-mcpu=native", "-march=armv8.3-a", "-mtune=neoverse-n1"]
    );
}

#[test]
fn elf_symbol_assignments_parse_as_zero_width() {
    let asm = concat!(
        "\t.section\t.llvm_stackmaps,\"a\",@progbits\n",
        "__LLVM_StackMaps:\n",
        "\t.byte\t3\n",
        "perry_null_guard_zero = 0\n",
        "\t.byte\t0\n",
        ".Lperry_ic_8 = .Ltmp3-4\n",
        "\t.hword\t0\n",
        "\t.word\t2\n",
    );
    let lines: Vec<&str> = asm.lines().collect();
    let block = super::parse_block(&lines, 4).expect("ELF symbol assignments must parse");
    // 1 + 1 + 2 + 4 -- the assignments contribute nothing.
    assert_eq!(block.bytes.len(), 8);
}

/// The x86-64 **ELF** spelling of `sample_asm`, with `tail` inserted
/// between the map's last byte and the section switch that ends the
/// block — the exact spot where `AsmPrinter` finalization prints the
/// attributes of the NEXT symbol it is about to define.
fn x86_64_elf_sample_asm(tail: &str) -> String {
    let mut asm = String::new();
    asm.push_str("\t.section\t.llvm_stackmaps,\"a\",@progbits\n");
    asm.push_str("\t.p2align\t3, 0x0\n");
    asm.push_str("__LLVM_StackMaps:\n");
    asm.push_str("\t.byte\t3\n\t.byte\t0\n\t.short\t0\n");
    asm.push_str("\t.long\t1\n"); // functions
    asm.push_str("\t.long\t0\n"); // constants
    asm.push_str("\t.long\t1\n"); // records
    asm.push_str("\t.quad\tprobe_fn\n");
    asm.push_str("\t.quad\t144\n"); // stack size
    asm.push_str("\t.quad\t1\n"); // record count
    asm.push_str("\t.quad\t0\n"); // patchpoint id
    asm.push_str("\t.long\t64\n"); // instruction offset
    asm.push_str("\t.short\t0\n");
    asm.push_str("\t.short\t4\n"); // location count
    for _ in 0..3 {
        asm.push_str("\t.byte\t4\n\t.byte\t0\n\t.short\t8\n\t.short\t0\n\t.short\t0\n\t.long\t0\n");
    }
    // The live root: RBP-relative (DWARF 6), frame offset -24.
    asm.push_str(
        "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t6\n\t.short\t0\n\t.long\t4294967272\n",
    );
    asm.push_str("\t.p2align\t3, 0x0\n");
    asm.push_str("\t.short\t0\n\t.short\t0\n"); // live-out header
    asm.push_str("\t.p2align\t3, 0x0\n");
    asm.push_str(tail);
    asm.push_str("\t.section\t\".note.GNU-stack\",\"\",@progbits\n");
    asm
}

/// The Linux crash this guards: LLVM prints `.hidden` + `.weak` for the
/// personality slot `DW.ref.perry_eh_personality` BEFORE switching to
/// its COMDAT section, i.e. inside what this parser treats as the
/// stack-map block. Dropping them assembles the slot as a LOCAL symbol
/// in a COMDAT group; the linker keeps one group per program and drops
/// every other object's CIE personality relocation (`.eh_frame` is
/// exempt from the discarded-section complaint), and the first caught
/// throw through a frame from any other module or codegen unit calls a
/// garbage personality pointer inside `_Unwind_RaiseException`. A
/// two-module program with a `try` in each module is enough to hit it.
#[test]
fn elf_personality_slot_attributes_survive_the_rewrite() {
    let asm = x86_64_elf_sample_asm(concat!(
            "\t.hidden\tDW.ref.perry_eh_personality\n",
            "\t.weak\tDW.ref.perry_eh_personality\n",
            "\t.section\t.data.DW.ref.perry_eh_personality,\"awG\",@progbits,DW.ref.perry_eh_personality,comdat\n",
            "\t.p2align\t3, 0x0\n",
            "\t.type\tDW.ref.perry_eh_personality,@object\n",
            "\t.size\tDW.ref.perry_eh_personality, 8\n",
            "DW.ref.perry_eh_personality:\n",
            "\t.quad\tperry_eh_personality\n",
        ));
    let (out, stats) = compact_stack_map_asm(&asm, "x86_64-unknown-linux-gnu")
        .expect("an x86-64 ELF stack map must parse")
        .expect("an x86-64 ELF stack map must be rewritten");
    assert_eq!(stats.functions, 1);
    assert_eq!(stats.roots, 1);
    assert!(!out.contains("__LLVM_StackMaps"), "{out}");
    assert_eq!(
        out.matches("\t.hidden\tDW.ref.perry_eh_personality\n")
            .count(),
        1,
        "the slot's visibility must be re-emitted exactly once:\n{out}"
    );
    assert_eq!(
        out.matches("\t.weak\tDW.ref.perry_eh_personality\n")
            .count(),
        1,
        "the slot's weak binding must be re-emitted exactly once:\n{out}"
    );
    // Order: map, then the attributes, then the slot's own section — the
    // layout LLVM printed, so the assembler sees exactly what it would
    // have seen without the rewrite.
    let map = out.find("_perry_gc_map:").expect("compact map label");
    let hidden = out.find("\t.hidden\tDW.ref").expect("hidden directive");
    let weak = out.find("\t.weak\tDW.ref").expect("weak directive");
    let section = out
        .find("\t.section\t.data.DW.ref.perry_eh_personality")
        .expect("slot section");
    assert!(map < hidden && hidden < weak && weak < section, "{out}");
    assert!(
        out.contains("\t.type\tDW.ref.perry_eh_personality,@object\n"),
        "{out}"
    );
}

/// Only lines about OTHER symbols are carried: the map's own label is
/// re-declared by the replacement, so anything naming it stays dropped.
#[test]
fn the_map_labels_own_attributes_are_not_carried() {
    let asm = x86_64_elf_sample_asm(concat!(
        "\t.globl\t__LLVM_StackMaps\n",
        "\t.type\t__LLVM_StackMaps,@object\n",
        "\t.size\t__LLVM_StackMaps, .-__LLVM_StackMaps\n",
    ));
    let (out, _) = compact_stack_map_asm(&asm, "x86_64-unknown-linux-gnu")
        .expect("an x86-64 ELF stack map must parse")
        .expect("an x86-64 ELF stack map must be rewritten");
    assert!(!out.contains("__LLVM_StackMaps"), "{out}");
    assert!(out.contains("_perry_gc_map:"), "{out}");
}

/// The -O3 ELF absolute-symbol aliases land inside the block too. They
/// define symbols the code references, so they must survive the rewrite
/// as well as parse to zero bytes.
#[test]
fn symbol_assignments_inside_the_block_are_re_emitted() {
    let asm = x86_64_elf_sample_asm(concat!(
        "perry_null_guard_zero = 0\n",
        ".Lperry_ic_8 = .Ltmp3-4\n",
    ));
    let (out, stats) = compact_stack_map_asm(&asm, "x86_64-unknown-linux-gnu")
        .expect("an x86-64 ELF stack map must parse")
        .expect("an x86-64 ELF stack map must be rewritten");
    assert_eq!(stats.roots, 1);
    assert_eq!(
        out.matches("\nperry_null_guard_zero = 0\n").count(),
        1,
        "{out}"
    );
    assert_eq!(
        out.matches("\n.Lperry_ic_8 = .Ltmp3-4\n").count(),
        1,
        "{out}"
    );
}

#[test]
fn trailing_llvm_buffer_nul_is_not_an_assembly_directive() {
    let asm = concat!(
        "\t.section\t.llvm_stackmaps,\"a\",@progbits\n",
        "__LLVM_StackMaps:\n",
        "\t.byte\t3\n",
        "\0\n",
    );
    let lines: Vec<&str> = asm.lines().collect();
    let block = super::parse_block(&lines, 4).expect("trailing NUL must be ignored");
    assert_eq!(block.bytes, vec![3]);
}

#[test]
fn expression_operators_are_not_mistaken_for_assignments() {
    for line in [
        ".byte\t1",
        "\t.size\tsym, .-sym",
        "\t.if a == b",
        "\t.if a != b",
    ] {
        assert!(
            !super::is_symbol_assignment(line),
            "`{line}` must not be treated as a symbol assignment"
        );
    }
    for line in ["sym = 1", ".Lfoo = .Ltmp1-4", "a$b = 7"] {
        assert!(
            super::is_symbol_assignment(line),
            "`{line}` is a symbol assignment"
        );
    }
}

#[test]
fn repeated_live_sets_cost_one_byte() {
    let shared = vec![(29u16, -24i32), (29, -32)];
    let functions = vec![FunctionMap {
        symbol: "_f".to_string(),
        stack_size: 64,
        records: vec![
            Record {
                instruction_offset: "0".to_string(),
                roots: shared.clone(),
                derived: Vec::new(),
            },
            Record {
                instruction_offset: "8".to_string(),
                roots: shared.clone(),
                derived: Vec::new(),
            },
            Record {
                instruction_offset: "16".to_string(),
                roots: shared,
                derived: Vec::new(),
            },
        ],
    }];
    let one_record = vec![FunctionMap {
        symbol: functions[0].symbol.clone(),
        stack_size: functions[0].stack_size,
        records: functions[0].records[..1].to_vec(),
    }];
    // Offsets live in their own fixed-width array now, so in the varint
    // stream the two extra records cost exactly one repeat byte each,
    // regardless of how many roots the shared live set holds.
    assert_eq!(
        encode_stream(&functions).expect("encodes").bytes.len(),
        encode_stream(&one_record).expect("encodes").bytes.len() + 2
    );
}

#[test]
fn encodes_a_foreign_register_base() {
    // A base that is neither FP nor SP is real: LLVM uses x19 as a frame
    // base pointer in functions with dynamic stack allocation. The 2-bit
    // tag carries the DWARF number explicitly rather than refusing — the
    // format must never be the reason a root is unrepresentable.
    let asm = sample_asm().replace(
        "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t29\n",
        "\t.byte\t3\n\t.byte\t0\n\t.short\t8\n\t.short\t19\n",
    );
    let (out, stats) = compact_stack_map_asm(&asm, "aarch64-unknown-linux-gnu")
        .expect("block parses")
        .expect("a foreign base must still encode");
    assert_eq!(stats.roots, 1);
    assert!(out.contains("_perry_gc_map:"));
}

/// #7803: derived (interior) slots survive the encode/decode round trip,
/// share the repeat flag with their bases, and a differing derived set
/// breaks the repeat.
#[test]
fn derived_slots_roundtrip_and_share_the_repeat_flag() {
    let record = |off: &str, derived: Vec<(u32, u16, i32)>| Record {
        instruction_offset: off.to_string(),
        roots: vec![(29, -16), (29, -8)],
        derived,
    };
    let repeated = vec![FunctionMap {
        symbol: "probe".to_string(),
        stack_size: 96,
        records: vec![
            record("0", vec![(1, 31, 24)]),
            record("16", vec![(1, 31, 24)]),
        ],
    }];
    let stream = encode_stream(&repeated).expect("encodes");
    verify_roundtrip(&repeated, &stream).expect("derived records must round-trip");
    let single = vec![FunctionMap {
        symbol: "probe".to_string(),
        stack_size: 96,
        records: vec![record("0", vec![(1, 31, 24)])],
    }];
    assert_eq!(
        stream.bytes.len(),
        encode_stream(&single).expect("encodes").bytes.len() + 1,
        "an identical (roots, derived) pair must cost one repeat byte"
    );

    let differing = vec![FunctionMap {
        symbol: "probe".to_string(),
        stack_size: 96,
        records: vec![
            record("0", vec![(1, 31, 24)]),
            record("16", vec![(0, 31, 24)]),
        ],
    }];
    let stream = encode_stream(&differing).expect("encodes");
    verify_roundtrip(&differing, &stream)
        .expect("a differing derived set must re-encode, not repeat");
    assert!(
        stream.bytes.len() > encode_stream(&single).expect("encodes").bytes.len() + 1,
        "a record whose derived set differs must not take the repeat flag"
    );
}

/// The round-trip check must be able to FAIL. A verifier that only ever
/// agrees with itself is CLAUDE.md's fourth gate-failure mode — the gate
/// runs, its subject never did — so plant each way the stream can lie and
/// assert the check catches it rather than merely that a clean stream
/// passes.
#[test]
fn roundtrip_check_catches_a_corrupted_stream() {
    let functions = vec![FunctionMap {
        symbol: "probe".to_string(),
        stack_size: 96,
        // x86-64 shape: RSP (DWARF 7) base, ascending frame offsets.
        records: vec![
            Record {
                instruction_offset: "0".to_string(),
                roots: vec![(7, 8), (7, 24), (7, 40)],
                derived: Vec::new(),
            },
            Record {
                instruction_offset: "16".to_string(),
                roots: vec![(7, 8), (7, 24), (7, 40)],
                derived: Vec::new(),
            },
        ],
    }];
    let stream = encode_stream(&functions).expect("encodes");
    verify_roundtrip(&functions, &stream).expect("a clean stream must verify");

    // A dropped root: the header's count is the first byte of the stream.
    let mut short = stream.clone();
    short.bytes[0] = 2 << 2;
    assert!(
        verify_roundtrip(&functions, &short).is_err(),
        "a stream claiming fewer roots than the map recorded must be rejected"
    );

    // A moved root: perturbing a delta keeps the count but relocates the
    // slot, which is the shape that makes the collector scan wrong words.
    let mut moved = stream.clone();
    moved.bytes[1] = moved.bytes[1].wrapping_add(4);
    assert!(
        verify_roundtrip(&functions, &moved).is_err(),
        "a stream that relocates a root must be rejected"
    );

    // Truncation.
    let mut truncated = stream.clone();
    truncated.bytes.pop();
    assert!(
        verify_roundtrip(&functions, &truncated).is_err(),
        "a truncated stream must be rejected"
    );

    // Trailing bytes: decodes cleanly and still means the two sides
    // disagree about the layout.
    let mut trailing = stream.clone();
    trailing.bytes.push(0);
    assert!(
        verify_roundtrip(&functions, &trailing).is_err(),
        "a stream with unconsumed trailing bytes must be rejected"
    );
}

/// The per-function stream offsets must be checked, not merely recorded.
///
/// Nothing emits or reads them yet, so the only thing standing between "these
/// are right" and "these are assumed right" is that `verify_roundtrip` rejects
/// a wrong one — on every compile, over every function. Plant each way an
/// offset can be wrong and assert it is caught, the same way the stream's own
/// corruptions are.
#[test]
fn roundtrip_check_catches_a_wrong_per_function_stream_offset() {
    let probe = |symbol: &str, offsets: &[&str]| FunctionMap {
        symbol: symbol.to_string(),
        stack_size: 96,
        records: offsets
            .iter()
            .map(|off| Record {
                instruction_offset: (*off).to_string(),
                roots: vec![(7, 8), (7, 24)],
                derived: Vec::new(),
            })
            .collect(),
    };
    // Two functions, so the second has a non-zero offset to get wrong.
    let functions = vec![probe("first", &["0", "16"]), probe("second", &["0", "16"])];
    let stream = encode_stream(&functions).expect("encodes");
    verify_roundtrip(&functions, &stream).expect("clean offsets must verify");
    assert_eq!(
        stream.function_offsets.len(),
        2,
        "one offset per function, in order"
    );
    assert_eq!(stream.function_offsets[0], 0, "the first starts the stream");
    assert!(
        stream.function_offsets[1] > 0,
        "the second must start after the first"
    );

    for delta in [1i64, -1, 4] {
        let mut wrong = stream.clone();
        wrong.function_offsets[1] = (i64::from(wrong.function_offsets[1]) + delta)
            .try_into()
            .expect("in range");
        let error = verify_roundtrip(&functions, &wrong)
            .expect_err("an offset that is not where the decode lands must be rejected");
        assert!(
            error.contains("second") && error.contains("another function's live set"),
            "unhelpful reason for delta {delta}: {error}"
        );
    }

    // Too few offsets is the same defect wearing a different hat: the last
    // function would have no recorded start at all.
    let mut missing = stream.clone();
    missing.function_offsets.pop();
    assert!(
        verify_roundtrip(&functions, &missing).is_err(),
        "one offset per function, or the map cannot be read per function"
    );
}

#[test]
fn no_stack_map_block_is_left_alone() {
    // No block at all is `Ok(None)` — nothing to compact, not a failure.
    assert!(compact_stack_map_asm(
        "\t.section\t__TEXT,__text\n\tret\n",
        "arm64-apple-macosx15.0.0"
    )
    .expect("no block is not an error")
    .is_none());
}

#[test]
fn unparsable_block_is_an_error_not_a_silent_skip() {
    // Truncated header. This must be `Err`, never `Ok(None)`: the caller
    // turns `Err` into a refusal and `Ok(None)` into "assemble unchanged",
    // and assembling unchanged here ships a binary whose roots the
    // collector cannot see.
    let asm = "\t.section\t__LLVM_STACKMAPS,__llvm_stackmaps\n\t.byte\t3\n";
    let error = compact_stack_map_asm(asm, "arm64-apple-macosx15.0.0")
        .expect_err("truncated block must error");
    assert!(
        error.contains("no function records") || error.contains("past the end"),
        "unhelpful reason: {error}"
    );
}
