**Stores to an ordinary `Array` element no longer re-prove a loop-invariant receiver on every element.**

An indexed write cost **105 instructions per element** — against 8 for the same store to a `Float64Array` and 12 for node — with zero runtime calls. **51 of the 105 were loop-invariant** receiver revalidation, and a further 42 was a write-barrier decision provable away from the value's type.

This widens the store admission the way #10731 widened reads. The gate was `has_materialization_hazard`, which a trailing `console.log` is enough to set.

`a[i] = k + i` **105 → 17.4**, `a[i] = a[i] + 1` **256 → 24.5** (node 18.7), `a[i] = a[i] + b[i]` **333 → 35.9**. The bare loop, both `Float64Array` paths and the indexed read are unchanged to the instruction.

Note this moves none of the five real programs in #10695 — their loop bodies are multi-statement or contain calls, which no current tier admits (#10741) — and `a[i] += 1` is unaffected because its lowering is two statements (#10743).
