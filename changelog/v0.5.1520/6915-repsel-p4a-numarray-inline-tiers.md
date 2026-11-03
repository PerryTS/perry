**Representation-selection Phase 4a — fast plain-array numeric elements (#6904)**

Repairs the `number[]` access path in three layers (RFC `docs/representation-selection-rfc.md`, new §4/§5.7 `Array<number>` rows):

- **4a.0 inference**: `is_numeric_expr` gains the missing `Expr::Logical` arm (plus the matching boxed-fallback-hazard arm), and number-context `&&`/`||`/`??` lower with real-double operands — `(counts[v] || 0) + 1` now compiles to `fcmp one` + select + `fadd` instead of `js_is_truthy` + `js_dynamic_string_or_number_add`. `??` keeps its nullish test on the uncoerced value (`NaN ?? x` stays `NaN`). New LLVM attribute group `#4` (`nounwind willreturn`) for the audited array index/push guards.
- **4a.1 inline guard tiers**: the numeric read, write, and push paths get the inline structural guard the untyped tier had (header-byte tests, no out-of-line call on the fast path), ending the typed-`number[]`-slower-than-untyped inversion in both directions. Canonical-by-construction stores skip `js_array_numeric_value_to_raw_f64` entirely.
- **4a.2 holes axis**: number-context reads accept the raw-f64-or-holes invariant with a proof-gated 2-instruction NaN-canonicalization (bit-exact with `ToNumber(undefined)`/`ToNumber(NaN)`); the write tier gap-fills sparse extends inline with a dense→holes header transition; and `js_array_set_f64_extend` no longer permanently demotes sparsely-extended numeric arrays (its own `TAG_HOLE` gap stores previously cleared the layout flags). Hole-vs-undefined observability (`in`/`Object.keys`/`JSON.stringify`) is byte-exact throughout.

Also fixes a latent Phase 2 interaction: a specialized-ABI callee growing a caller-allocated array left the caller's binding on a pre-growth forwarded stub, pinning every access (including the pre-existing packed-loop guards) to the boxed chain-following fallback. The guard tiers' cold arms now self-heal the binding via `js_array_refresh_local_head`.

Deterministic #6904 histogram benchmark added (`benchmarks/bench_histogram_numarray.ts`); three new gap tests + runtime unit tests. The 4a.3 `Ptr<NumArray>` collector (guard-free consumers) is documented in the RFC and follows separately.
