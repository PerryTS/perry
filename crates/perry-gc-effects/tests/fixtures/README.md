These are the C fixtures from scripts/gc_call_effects/self_test.py, built
with LLVM 22 on Linux. They need no SDK headers or link step. Checking in
these small archives keeps cargo test -p perry-gc-effects independent of
Clang, llvm-ar, and cross SDK availability on CI.

Each format has a normal and planted archive (PLANT becomes
x = js_proxy_get(x);), plus the production hot_text.c body compiled for
that target. Member names preserve cc::Build's 8a1b2c3d-hot_text.o convention.
The tests assert 12 helpers' classes/throws, non-vacuous switch dispatch,
native source ownership, all four solves, witness paths, unsafe table drift,
dead named rules, forbid violations, and solve agreement.

Rebuild from the repository root with:
python3 crates/perry-gc-effects/tests/fixtures/rebuild.py

Targets: x86_64-unknown-linux-gnu (ELF),
arm64-apple-macos11 (Mach-O), x86_64-pc-windows-msvc (COFF).

COFF TimeDateStamp fields are normalized to zero by rebuild.py.
