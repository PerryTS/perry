### Pin `apple-m1` instead of `-mcpu=native` on Apple aarch64

`gc-native-roots` has never gone green — **0 successes in 40 runs** — and the
current cause is not a GC bug at all:

```
fatal error: error in backend: Cannot select: intrinsic %llvm.aarch64.fjcvtzs
```

`inprocess.rs` already documents the invariant this breaks. Codegen decides
whether to emit `llvm.aarch64.fjcvtzs` (FEAT_JSCVT, the single-instruction
ECMAScript `ToInt32`) **from the triple alone**, because clang's default CPU for
`arm64-apple-*` is `apple-m1`. Anything that then compiles that IR for a CPU
without the feature aborts. The doc calls out the generic-TargetMachine half of
that pair; `-mcpu=native` is the other half, and it fails the same way wherever
CPU detection disagrees with the triple assumption — which is exactly what a
virtualised macOS CI runner does. The identical command works on a physical Mac,
which is why this only ever failed in CI.

Apple aarch64 hosts now pass an explicit `-mcpu=apple-m1` rather than `native`,
making what Perry emits and what it targets the same decision instead of two
that happen to agree on developer hardware. Every other host keeps native
tuning.

Verified on hardware: `native_tuning_arg = -mcpu=apple-m1` in the recorded
compile plan, and all ten gc-ratchet probes still byte-match the pinned Node
oracle under `PERRY_RS4GC=1 PERRY_GC_FORCE_EVACUATE=1
PERRY_GC_VERIFY_EVACUATION=1`.
