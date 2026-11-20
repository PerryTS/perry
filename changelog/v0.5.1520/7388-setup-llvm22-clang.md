**Fixed** `setup-llvm22` installed LLVM 22's development libraries without
`clang-22`, leaving `/usr/lib/llvm-22/bin/opt` with no clang beside it.

apt ships opt and clang as separate packages, so `llvm-22-dev` alone produces a
half-populated bin directory — the state `ubuntu-24.04-arm` runners land in.
Consumers that need a *matched* opt+clang pair (the RS4GC arm in
`gc-native-roots.yml`) then either fail the pair-check outright or fall back to
hand-rolled discovery and install the distro's unversioned `llvm clang`, which on
Ubuntu 24.04 is LLVM 18 — running `opt` 18 over IR emitted by Perry's linked
LLVM 22.

The action now installs `clang-22`, symlinks it under the `llvm-config-22`
prefix so directory-based toolchain resolution finds a co-located pair, and
asserts clang's major version alongside the existing `llvm-config` check. A green
setup step that leaves a mismatched clang is precisely the failure this action
exists to prevent.

Fixes the root cause of #7384 for all 18 workflows rather than one.
