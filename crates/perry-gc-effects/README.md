# perry-gc-effects

Derives the GC and unwind effects of every exported runtime symbol from the
machine code of the runtime archives. Reads ELF, Mach-O and COFF archives
with object and disassembles x86-64/AArch64 with Perry's LLVM 22 C API. Rules
are compiled from scripts/gc_call_effects/seeds.txt using include_str!. There
is no effect cache or alternate classification path. Section decoding and
independent rule matching use at most eight workers; graph resolution and
output order follow archive order.

Build with the same LLVM 22 development installation as perry-codegen
(LLVM_SYS_221_PREFIX when necessary). Windows uses LLVM-C.lib/LLVM-C.dll,
matching perry-codegen; other hosts use llvm-sys's normal static linkage.

    cargo test -p perry-gc-effects
    scripts/gc_call_effects/sidecars.sh target/release

`sidecars <lib-dir>` is how the tables are made: it writes
`<archive>.gc-effects` beside every runtime archive in the directory (the
runtime alone, and with its stdlib), keyed by the size and SHA-256 of the
analyzed bytes. Every recipe that builds runtime archives runs it, and perry
reads the sidecar of the archive it links (nothing is checked in). The rule
checks (solves agree, forbid premises, no dead seed rules, the symbol floor)
are hard errors there.

`why --archive <a> [--archive <b>] <symbol>...` prints a witness path per
solve: why a symbol is not Leaf, or can throw. `generate` writes a single
table file (the fixture tests use it). --rules replaces the compiled rules
for tests/review; --root selects the ownership-scan source tree (defaults to
this crate's workspace).

The pinned facts about the real runtime (hot helpers that must stay Leaf,
#11522, #11523, the polls, the root-dominance checker's authority) are
checked over a freshly built workspace sidecar:

    PERRY_GC_EFFECTS_SIDECAR=target/release/libperry_runtime.a.gc-effects \
        cargo test -p perry-gc-effects --test workspace_sidecar -- --ignored

The fixtures README documents the checked-in object fixtures and their
rebuild command. Tests use only the library and binary, with no subprocess
disassembler, nm, demangler, Python, Clang, archiver or cross SDK required.
The optional inspect/profile examples support compatibility and cost audits.
