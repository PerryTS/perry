# perry-gc-effects

Compatibility port of scripts/gc_call_effects/callgraph.py. Reads ELF,
Mach-O and COFF archives with object and disassembles x86-64/AArch64 with
Perry's LLVM 22 C API. Rules are compiled from seeds.txt using include_str!.
The four reverse solves, alias rules, source ownership/shadowing scan and
hard checks retain Python's behavior. There is no effect cache or alternate
classification path. Section decoding and independent rule matching use at
most eight workers; graph resolution and output order follow archive order.

Build with the same LLVM 22 development installation as perry-codegen
(LLVM_SYS_221_PREFIX when necessary). Windows uses LLVM-C.lib/LLVM-C.dll,
matching perry-codegen; other hosts use llvm-sys's normal static linkage.

    cargo test -p perry-gc-effects
    cargo run --release -p perry-gc-effects -- generate --target linux-x86_64         --out fresh.tsv target/release/libperry_runtime.a target/release/libperry_stdlib.a
    scripts/gc_call_effects/regen.sh linux-x86_64 --check --cross-check

The standalone CLI also has check, why, and lint commands. --rules replaces
the compiled rules for tests/review; --root selects the ownership-scan source
tree (defaults to this crate's workspace). check accepts --write-fresh and
--allow-safe-drift with the same missing-symbol semantics as Python.

regen.sh uses the Rust CLI. Its optional --cross-check mode runs both tools
and diffs complete TSV bytes before generating/checking the committed table.
GC_EFFECTS_BIN can select an already-built binary. Keep callgraph.py,
self_test.py and why.py for one release as the independent oracle. The TSV
header deliberately retains its original Python generator spelling so that
output remains byte-identical; no checked-in effect table changes here.

The fixtures README documents the checked-in object fixtures and their
rebuild command. Tests use only the library and binary, with no subprocess
disassembler, nm, demangler, Python, Clang, archiver or cross SDK required.
The optional inspect/profile examples support compatibility and cost audits.
