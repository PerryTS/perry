# Native Representation

This document describes the H1 native-representation path from HIR facts to
LLVM emission. The current scope is intentionally narrow: it records and checks
when values stay in a native representation instead of being materialized as a
generic JavaScript `double`/NaN-box value.

## Pipeline

1. HIR fact collectors identify local facts such as fresh Buffer ownership,
   integer loop counters, min-length bounds, aliases, reassignment, closure
   capture, and unknown-call escape.
2. Expression lowering produces a `LoweredValue`: the JavaScript semantic kind,
   the selected `NativeRep`, the LLVM type, and the SSA value.
3. A `NativeRep` describes the compiler contract, not an optimization by
   itself. Examples are `I32`, `U32`, `U64`, `USize`, `F32`, `F64`, `U8`,
   `BufferLen`, `NativeHandle`, `PromiseBoundary`, `JsValueBits`, `JsValue`,
   and `BufferView`.
4. `materialize_js_value` is the boundary where a native value is converted back
   to the generic JS ABI representation. Each conversion records a
   `MaterializationReason` and, for native ABI crossings, a
   `native_abi_transition`.
5. LLVM emission consumes the native representation. For buffers, unsafe LLVM
   attributes are emitted only through `BufferAccessProof`.
6. `native-reps.json` records what happened: source function, lowering block,
   optional stable `region_id`, native rep, bounds state, alias state,
   access mode, materialization reason, native ABI transition, and whether
   `inbounds` or alias metadata was emitted.
7. The compiler-output harness verifies both optimized IR shape and
   native-representation records. H1 fixtures use labeled loops so checks can
   match exact `region_id`s instead of optimized LLVM block names.

## H1 Uint8Array/Buffer Path

For `Uint8ArrayGet`, `Uint8ArraySet`, `BufferIndexGet`, `BufferIndexSet`, and
Buffer numeric reads, the fast path is:

1. Resolve the receiver to a tracked `BufferViewSlot`.
2. Compute `BoundsState` from loop/min-length facts without lowering the index.
3. If bounds are unknown, route through the dynamic runtime fallback.
4. Lower the index as an i32 `LoweredValue`.
5. Compute effective `AliasState` from the slot and function-local buffer views.
6. Build a `BufferAccessProof`.
7. Emit data-pointer load, length load, optional assume, GEP, and load/store from
   that proof.

`proof.may_emit_inbounds` is the only source of truth for
`getelementptr inbounds`. `proof.may_emit_noalias` is the only source of truth
for `!alias.scope` / `!noalias` metadata. Both are derived only for
`unchecked_native` accesses with proven or guarded bounds. Multi-byte numeric
reads route through the dynamic fallback unless a future width-aware proof is
added.

`access_mode` is serialized in schema version 3:

- `unchecked_native`: raw native GEP/load/store; requires proven or guarded
  bounds.
- `checked_native`: reserved for a future branch-and-fallback lowering; verifier
  requires proven or guarded bounds before it can appear.
- `dynamic_fallback`: runtime helper or generic dispatch path.

## Selected Native ABI / Region-Local Contract

Schema version 12 records explicit native ABI transitions, internal boxed bits
counts, and the LLVM ABI type for values that stay native within a verified
region:

Scope note: these records do not describe a general typed function, method, or
closure ABI. User-defined function and method signatures still use the generic
`double`/NaN-box ABI at call boundaries, and closure bodies still use
`i64 this_closure` plus `double` arguments and return `double`. The contract
covers selected native binding adapters and optimizer-local native values inside
a verified function/region.

- `I32`, `U32`, and `BufferLen`: LLVM `i32`; `U32` and `BufferLen` materialize
  with unsigned integer-to-double conversion.
- `I64`, `U64`, `USize`, `NativeHandle`, and `PromiseBoundary`: LLVM `i64`;
  `U64` and `USize` materialization is unsigned and lossy above JS integer
  precision.
- `F32`: LLVM `float`; JS-number materialization is explicit `fpext` to
  `double`. Raw `f32` records are not JS-visible.
- `F64` and `JsValue`: LLVM `double`.
- `JsValueBits`: LLVM `i64`, used only as an internal NaN-box bit-pattern
  representation. Public ABI records still use `JsValue`/`double`.
- `BufferView`: LLVM `ptr`, scoped to the native buffer proof region.

`native_abi_transition` records use `{ from_native_rep, to_native_rep, op,
reason, lossy }`. Valid ops are `none`, `signed_int_to_float`,
`unsigned_int_to_float`, `float_extend`, `js_value_to_bits`,
`bits_to_js_value`, `pointer_box`, `native_handle_box`, and `promise_box`.
The `js_value_to_bits` and `bits_to_js_value` ops are plain bitcasts that mark
the boundary between the current `double` ABI and the optimizer-local boxed
bits representation. The legacy `scalar_conversion` field is still written for
compatibility, but new checks should read `native_abi_transition`.

## Verification Mode

Native-region verification is explicit. Enable it with
`--verify-native-regions` or `PERRY_VERIFY_NATIVE_REGIONS=1`. The CLI disables
the per-module object cache in this mode so lowering always runs. Artifact
writing is reporting-only: `PERRY_NATIVE_REPS=1` and `PERRY_LLVM_KEEP_IR=1`
write records but do not own verification.

The verifier rejects records that claim:

- `inbounds` without proven or guarded bounds.
- `noalias` without proven or guarded alias state.
- `unchecked_native` with unknown bounds.
- `checked_native` without proven or guarded bounds.
- `explicit_assume` as a bounds proof.
- LLVM type mismatches for the claimed native rep.
- JS-visible or materialized raw `F32` records.
- `JsValueBits` used as an external ABI descriptor or dynamic fallback record.
- Materialized `JsValueBits` records without a `js_value_to_bits` transition.
- Escaping raw `NativeHandle` or `PromiseBoundary` records.
- Native ABI transitions without a matching materialization reason.
- Invalid transition ops or signedness, including implicit unsigned/signed
  widening or narrowing claims.

## Buffer Fast-Path Benchmarking

Use `--disable-buffer-fast-path` or `PERRY_DISABLE_BUFFER_FAST_PATH=1` to force
tracked Buffer/Uint8Array accesses through the existing helper fallback. The
central buffer access proof returns `None` in this mode, so call sites exercise
the same slow path they use when a receiver has no tracked `BufferViewSlot`.

The `h1_buffer_fastpath_bench` compiler-output workload is intended for simple
A/B captures:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload h1_buffer_fastpath_bench \
  --runs 5 \
  --out-dir target/compiler-output-regression/h1_buffer_fastpath_bench_fast

PERRY_DISABLE_BUFFER_FAST_PATH=1 python3 scripts/compiler_output_regression.py capture \
  --workload h1_buffer_fastpath_bench \
  --runs 5 \
  --out-dir target/compiler-output-regression/h1_buffer_fastpath_bench_slow
```

## Extension Points

- `Uint32Array`: add a typed-array element kind and width-aware buffer access
  proof before emitting inbounds for multi-byte accesses.
- Strings: model string handles separately from generic `JsValue`, then record
  string-specific materialization at runtime API boundaries.
- JSON tape: represent parsed tape pointers and lengths as a native view with
  its own bounds facts.
- Scalar-replaced objects: record field-level native reps and materialize only
  when an object identity, dynamic property access, or escape requires it.
- Typed clones/trampolines: add a real typed function/method/closure ABI only
  when clone generation, generic trampoline dispatch, method call routing, and
  closure capture/call lowering are implemented together.
