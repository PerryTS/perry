### Native GC roots: refuse non-aarch64, and stop the ELF map forcing DT_TEXTREL

The `gc-native-roots` gate went red on `main` with a **SIGSEGV**, not a missing
section — `SHF_GNU_RETAIN` did keep `.perry_gcmap` through `--gc-sections`, and
`.llvm_stackmaps` was gone as intended. Two separate defects behind the crash.

**The backend is aarch64-only and did not say so.** Measured by cross-compiling
a probe to `x86_64-unknown-linux-gnu` and decoding the emitted stack map: every
one of its 178 root slots is `Indirect [RSP + off]`, DWARF register 7. The
runtime's `chain_walkable` admits only aarch64's FP/SP (29 and 31), so every
frame falls back to the unwinder, which resolves the base with
`_Unwind_GetGR(ctx, 7)` — and that does not reliably return the stack pointer
(`_Unwind_GetCFA` is the supported way). The walker therefore computed wild
addresses and the collector segfaulted writing through them. The mode is
opt-in, so the compiler now refuses the combination outright rather than
emitting a binary that crashes under collection.

**The ELF section was read-only but holds relocated addresses.** `ld` reported
`relocation against 'main' in read-only section '.perry_gcmap'` and created a
DT_TEXTREL in a PIE. It is now `"awR"` (SHF_ALLOC | SHF_WRITE | SHF_GNU_RETAIN).

The gate moves to an ARM64 Linux runner. On x86-64 it would now exercise only
the refusal; on ARM64 it tests the supported configuration and still answers
the question it exists for — whether the compact map survives ELF linking.
