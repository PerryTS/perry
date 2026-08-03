The compact-map rewriter could not read the stack maps **ELF** backends emit,
because `.word` is not a fixed size.

LLVM chooses each field's spelling per target through
`MCAsmInfo::Data32bitsDirective`, and the AArch64 **ELF** backend chooses
`.word`. GNU `as` defines `.word` as the target's natural machine word — 4 bytes
on AArch64, ARM, PowerPC, MIPS, SPARC and RISC-V, but **2 on x86**. The rewriter
carried one fixed table (`".long" | ".word" => 4`) written against the
Mach-O/AArch64 spelling it was developed on, so an `aarch64-unknown-linux-gnu`
map — where every 32-bit field is `.word`, every 16-bit field `.hword` and every
64-bit field `.xword` — was read at the wrong widths. The width is load-bearing
for the whole block: two bytes of drift per field relocates every root after it.

`.word` is now resolved against the target, and the other spellings an
`MCAsmInfo` can pick (`.1byte`/`.2byte`/`.4byte`/`.8byte`/`.dc.*`) are handled.

A directive inside the block whose width is not modelled is now a **refusal that
names it**, rather than being skipped. Skipping was the unsound branch: the
block is decoded by structural offset, so one ignored directive that emits bytes
shifts everything after it, and the decode then either fails somewhere unrelated
or succeeds against the wrong bytes. Naming it is what turned an opaque refusal
into a one-line diagnosis.

Every refusal now carries a reason — which directive, which record, which byte
offset, whether the per-function record counts disagreed with the header — and
the target. Previously every parse failure collapsed to `None`, so the message
could only repeat that it had failed, which is why #7321 took an issue to
localise.

The re-encode is now verified against the map it came from, on every target:
`verify_roundtrip` decodes the emitted varint stream exactly as
`perry-runtime`'s `parse_gc_map` does and asserts it reproduces every record's
live set. Unlike `PERRY_STACKMAP_WALKER=verify` this needs no
architecture-specific stack walker, so it holds where that check cannot run, and
it is sabotage-tested (dropped root, relocated root, truncation, trailing bytes)
so a pass means the detector works rather than that nothing was tried.

Also fixes an aarch64-ELF link failure the above uncovered: `eh_walker`'s
`global_asm!` defined `perry_eh_capture_context` / `perry_eh_install_context`
with Mach-O's leading underscore unconditionally under
`target_arch = "aarch64"`, so on aarch64 ELF the definitions and the
`extern "C"` declarations were different symbols and `perry-runtime` could not
link at all.

Measured on `aarch64-unknown-linux-gnu`, which could not compile a single module
under `PERRY_STATEPOINTS=1` before this and now runs the probe matrix **8/8**
against the pinned Node oracle under
`PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`, with `.perry_gcmap`
present and `.llvm_stackmaps` absent asserted per probe. (`09_try_catch_roots`
is excluded because the explicit bridge refuses invokes since #7330, on every
target.) Census over those eight: **478 statepoints, 0 plain stack maps, 0
parser fallbacks**, 648 relocations, 605 non-safepoint calls skipped, max 3 live
roots at one safepoint.

This does **not** close #7321, and the suspicion recorded there — that
`gc_map.rs`'s aarch64 register naming was the cause — is now measured to be
wrong. x86-64 stack maps parse: every root is `Indirect [RSP + off]`, DWARF
register 7, which round-trips through the compact format's explicit-register tag
exactly, and no configuration reproduced a parse failure (Apple clang 21,
Homebrew clang 19/20/22, Ubuntu clang 16/17/18, twelve `-march` settings, both
hosts, all nine probes). The x86-64 defect is one layer down, at collection time,
and #7324 refuses that target for it.

Filed while measuring this: **#7333** — `PERRY_STACKMAP_WALKER=unwind` segfaults
on Linux/ELF for 3 of the 8 probes, on a build whose default walker runs all 8
clean. It matters beyond a bisection control, because on x86-64 the fp-chain
walker is not compiled in and the unwinder *is* the walker.
