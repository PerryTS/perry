Perry can emit wasm32 code for the standalone WASI target (`--target wasi`,
`wasm32-unknown-wasip2`) when the compiler is built with the new,
off-by-default `target-wasi` cargo feature (phase 3b of #11375, #11378).
Without the feature nothing compiles differently. With it, only wasm32 WASI
takes any new path; host IR from a baseline, the default build and a
`target-wasi` build is otherwise unchanged (125/130 gap modules byte-identical,
and the rest differ only in an ordering the baseline itself varies).

- A wasm32-only pass rewrites each runtime declaration to the runtime's real
  signature. Every call whose own type differs goes through an inlined adapter.
  On LP64, codegen's i64-for-pointer convention matches the runtime; on wasm32
  it would be a link-time signature mismatch at ~1,200 sites.
- The signatures come from `crates/perry-codegen/src/wasm32/runtime_abi.tsv`.
  `scripts/runtime_abi_check.py --emit-wasm-abi` generates it from the runtime
  source, and `--check-wasm-abi` checks it is current.
- The shadow-frame push reads the 32-bit `frame_top` on wasm32.
- Inline shadow-slot stores, the Map entry fast path and inline arena
  allocation take their runtime calls on wasm32.

`runtime_abi_check.py --ir` verifies emitted wasm32 IR (27,946 runtime calls
across 54 gap modules, 0 mismatches). `scripts/wasi_ir_check.sh` runs it on
sample programs in the non-required `wasi-check` workflow's new
`wasi-codegen` job. Linking a `.wasm` is phase 4 (#11379).
