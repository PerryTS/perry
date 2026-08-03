### Fixed

- **`PERRY_RS4GC=1` now works on a stock toolchain.** RS4GC ran as an external
  `opt` subprocess whose output was handed to `clang`; on a Mac that pairing is
  Homebrew LLVM 22 feeding Apple clang 21, and the newer `opt` emits attributes
  the older `clang` rejects (`error: unterminated attribute group`). The knob was
  reachable only with `PERRY_LLVM_CLANG` pointed at a version-matched LLVM 22.

  That was load-bearing rather than cosmetic: RS4GC is the only native-root
  backend that can root an `invoke`, and since #7302 every call inside a `try` is
  an invoke — the explicit bridge refuses them (#7330), and 26% of the gap suite
  contains `try {}`.

  The pass now runs in-process (#7301), where LLVM 22 is already pinned and no IR
  crosses a toolchain boundary. Two gaps had to be closed to get there: the
  in-process backend discarded `-S` and returned an object where the statepoint
  backends asked for assembly, and nothing assembled the result — #7314's
  compact-map rewriter works on assembly text, so the assembly went into a `.o`
  and the link failed with `ld: unknown file type`.

  All nine `gc_ratchet` probes now compile with no `PERRY_LLVM_*` pinning,
  including `09_try_catch_roots`, which the bridge cannot compile at all. All
  nine are byte-identical to the shadow-stack control and copy 5,946–90,275
  objects under `PERRY_CONSERVATIVE_STACK_SCAN=off`.

  No default changes: `llvm-inprocess` is still a non-default cargo feature, and
  `PERRY_RS4GC=1` without it still takes the external path and still fails loudly.

- **`gc-native-roots` gained the arm that can observe the above.** Every existing
  RS4GC step pins `PERRY_LLVM_OPT` and `PERRY_LLVM_CLANG`, so none of them could
  see that the pinning became unnecessary. The new step is the only one that
  unsets both, and asserts the map section exists, the compact rewrite ran, a
  copying minor actually copied, and RS4GC — not a per-function bail to the
  bridge — did the lowering.
