**The numeric-window add kernel is one fused pass with a resume contract, and
`bench_numeric_array_downgrade` now beats node** (16 ms → 3 ms against node's
4 ms on an idle machine; checksums identical).

The mixed-layout tier for `arr[i] = arr[i] + delta` over `any[]` windows
(`match_numeric_range_add_loop`) was doing its job — the diagnosis that led
here first established that the benchmark's 3.6× was almost entirely the
declared-type effect and not the heterogeneity, then that the loop was already
claimed by this purpose-built tier. The whole cost sat in its runtime kernel:
two full passes over a 1MB window (validate every slot, then mutate every
slot) with a branchy NaN-box decode per slot per pass, ~2.9 ns per element
against node's 0.64.

The all-or-nothing contract those two passes bought was stronger than the
source semantics require. Each element receives exactly one `+ delta` whether
the kernel or the ordinary loop applies it, so mutating up to the first
non-number and letting the ordinary loop resume there is observably
identical. The kernel now returns `>= 0` (window done), `-1` (receiver-level
decline, nothing mutated), or `<= -2` (slots `[start, k)` updated, resume at
`k = -ret - 2`); the lowering seeds the counter with the resume index before
entering the fallback loop — safe because `lower_for` lowers the init before
any matcher runs, so the fallback cannot re-run it and double-apply. The
double lane is decoded first (after the first call every slot holds a boxed
double); the int lane keeps the class-ref exclusion in the shared decoder.

The resume path is pinned by tests the benchmark itself never exercises,
since its windows are entirely numeric: a non-number mid-window gets node's
exact semantics — one increment per element, concatenation where `+`
concatenates (`"[object Object]1"`, `"mid1"`), NaN slots staying NaN — under
normal and forced-evacuation runs.

Also names the packed-f64 versioned matcher's one silent gate
(`no_length_hoist`): a loop whose bound is not `arr.length` exited before any
named reject could fire, which made every literal- or parameter-bounded loop
invisible to `PERRY_PACKED_LOOP_TRACE` — the gap that forced this diagnosis
through binary instrumentation instead of one trace run.
