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

This does **not** yet close #7321. The defect is an ELF defect but specifically
an AArch64-ELF one; x86 ELF spells these fields `.byte`/`.short`/`.long`/`.quad`,
which the old table already handled, and the x86-64 refusal could not be
reproduced under Apple clang 21, Homebrew clang 19/20/22 or Ubuntu clang 18,
across twelve `-march` settings, from either host, over all nine probes.
