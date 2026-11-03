**`16_matrix_multiply` is now twice as fast as node** (100 ms → 16 ms against
node's 32 on an idle machine; checksums identical) — the last benchmark in
the suite crosses parity, via two changes and one guard fix.

**The affine window is proven at the loop's endpoints.** An index tree linear
in the counter takes its extremes at the interval's ends, so the entry guard
evaluates each recorded tree at `start` and at `bound − 1` (wrap-free by the
magnitude bound) and unsigned-compares both against the live length — two
compares per tree, once per loop entry, for any coefficient sign. Reads under
a proven window drop both the range clamp and the per-read bounds check,
leaving a bare `trunc` and raw load. Non-linear trees (`k * k`) keep their
per-read checks.

**The accumulator walk takes the guarded array set.** The old single-array
restriction was why `sum = sum + a[..] * b[..]` never earned its number
proof: every k-iteration wrote `sum` through a boxed shadow-slot store with a
barrier, which profiling showed was the actual bottleneck once the guards
were hoisted — removing the per-read checks alone moved nothing. Every array
in the set is validated by the same AND-reduced entry guard, so a read of any
of them inside the clone is a Number by the same argument that held for one;
affine reads qualify as numeric leaves through the same shared predicate the
matcher and the lowering use (`affine_leaf_admissible`), so the three cannot
drift.

**A guard hole in the #9294 arm is closed.** Its receiver-only `continue`
also fired for arrays with BOTH counter-offset and affine accesses, skipping
the windowed guard while the counter fact still said `window_validated:
true` — `a[k + 1]` alongside `a[i * size + k]` then read one raw slot past
the loop's window at the boundary. Mixed arrays now fall through to the
windowed guard, with the affine endpoint proof appended to either path, and
a test pins the mixed shape to node's NaN under both collector modes.
