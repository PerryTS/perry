perf: array read-modify-write in a multi-statement loop body runs on the dense range tier (#10718)

`for (let i = 0; i < N; i++) { a[i] = a[i] + 1; s += a[i]; }` fell off every
loop tier. The classic range mode admits only one statement, because a side exit
after a store would replay the store. The dense mode, which has no side exits,
admitted only masked `a[e & K]` stores. The loop therefore ran on the generic
path and re-validated the receiver for each of its three accesses: 206
instructions per element against node's 39.

The dense mode now admits counter-offset stores (`a[i] = …`, `a[i ± c] = …`)
under the same rule as its masked stores. The entry guard validates the whole
counter window (in bounds, hole-free, raw-f64, plain, integrity-clean), and the
stored value must be a statically genuine double: a literal, the counter, a
guarded element read, or `+ - * /` and negation over those. So the store needs
no value check and no side exit, and an iteration still runs entirely in one
copy. The compound-assignment alias fold (#10743) now also runs over
multi-statement bodies, so `a[i] += 1; s += a[i]` takes the same path. The
matcher verifies accumulators with the same array set the lowering uses, which
lifts the old single-array restriction (`a[i] = a[i] + b[i]; s += a[i]`).

Result: `arrRmw` goes from 206 to 14.6 instructions per element (node 39.2), and
the single-statement form goes from 24.5 to 20.5. Tests: IR tests in
`stmt/range_loop_dense_store_tests.rs`. Removing the genuine-RHS rule turns
`declines_a_store_whose_value_is_not_provably_a_double` red (verified by
sabotage). The gap test `test_gap_10718_array_rmw_dense.ts` covers holes,
deleted elements, out-of-bounds windows, numeric strings, BigInt, `valueOf`,
element-kind changes between loop entries, frozen arrays, aliasing, and
-0/NaN/Infinity.
