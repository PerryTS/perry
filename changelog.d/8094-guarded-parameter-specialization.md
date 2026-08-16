### Performance

- Recover ordinary typed-parameter optimization in runtime-guarded function clones while preserving the conservative generic fallback for erased or lying TypeScript annotations (#8079, #8094).

  A guard-eligible module function now has three symbols: a `noinline` routing
  trampoline keeping the public name and the JSValue ABI, the unchanged
  `$generic` body, and a `$spec_*` clone that receives parameter proofs only
  after `js_param_type_guard` (or a raw scalar guard) accepted the live
  argument at that entry. Reference-typed parameters lose their proof across
  any call in the body, because unknown code can reach the same object through
  an alias the caller arranged before entry.

### Fixed

- `has_any_mutation` was gating the SPEC-ABI `demoted` mask, which also drives
  raw representation selection, so writing through a parameter (`values[i] = v`)
  demoted its raw slot and deleted specializations that predate this work — a
  `Float64Array` fill lost its `$spec_ta…_i32` entry entirely. Content mutation
  invalidates a descriptor PROOF, not a calling convention, so it moved to the
  guard-only `guard_blocked` mask. The emitted guard set is unchanged by
  construction (the two masks are OR-ed in the same predicate); only the
  representation is restored (#8094).
