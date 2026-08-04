### Windows: COFF section and PE lookup land; the walker is what is still missing

Two of the three pieces Windows needs are in, and the third is named rather
than glossed.

**The section.** COFF joins Mach-O and ELF as a first-class object format in
the map emitter. Its name is `.pgcmap`, not `.perry_gcmap`, and that is
load-bearing: a **PE image section header has an 8-byte name field**, and long
names survive only in object files as a string-table offset the linker does not
carry into the image. A 12-byte name would be truncated on the way in and the
runtime could never match it.

**The lookup.** The runtime can find that section in a running PE image —
`GetModuleHandleW(NULL)` gives the image base, which is also the
`IMAGE_DOS_HEADER`; the section table follows the optional header, whose size
the file header records rather than being fixed.

**The walker is missing, so Windows stays refused.** `_Unwind_*` does not exist
there, and the walker module is gated to Apple and Linux — on Windows it falls
to the stub, no frame is ever visited, and the collector would free live
objects. Emitting a map anyway would produce exactly the silent-lost-roots
failure this backend exists to make impossible, so the compiler refuses the
target with a message that says which piece is absent.

A walker there means either `RtlVirtualUnwind`, or an fp-chain walk (Perry
forces frame pointers, so RBP does chain) — and it wants a Windows host to
develop against, which is why this lands staged rather than half-enabled.

Verified: the PE lookup compiles for `x86_64-pc-windows-msvc` in isolation. The
full crate cannot be cross-checked from macOS because `psm`/`stacker` build
scripts need a C cross-compiler — the same blocker that stops local watchOS and
visionOS checks, unrelated to this code. CI's `windows-build` job covers it.
