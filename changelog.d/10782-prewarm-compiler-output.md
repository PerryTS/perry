Gave the `compiler-output-regression` job's cold auto-optimize runtime builds a
budget of their own, closing the residual #10782 left behind. The job is a
`main-gate` input, so while it was red every merge bypassed it.

`perry compile` resolves `optimized_libs` unconditionally
(`run_pipeline.rs:6169`), several hundred lines before `--no-link`'s early
return, so the first linking compile in the job blocks on a nested
`cargo build --release -p perry-runtime-static -p perry-stdlib-static` into
`target/perry-auto-<hash>/`. `PERRY_RUNTIME_DIR` does not suppress it. #10782
moved that cost off the harness's `--print-hir --no-link` probe, where
suppressing auto-optimize is inert, but deliberately not off the linking
compile: `runtime_budgets`' `allocations_traced` / `gc_collections_traced` /
`write_barriers_traced` are read out of the executed binary's `PERRY_GC_TRACE`
stderr and are MAXIMA, so a runtime emitting no trace scores 0 and passes all
three vacuously. The build therefore just moved into the linking compile's
300 s `--compile-timeout`, and the gate kept failing with `TimeoutExpired` on
`h1_native_rep_equivalence`, the head of both suites.

A `Pre-warm auto-optimized runtime` step now does those builds before the
gates, with `--skip-run --compile-timeout 2400`, so each gate step keeps its
300 s and keeps measuring exactly what it measured before.

**It is two cold builds, not one.** Every gated workload shares a single
`target/perry-auto-<hash>/` — the directory hash keys on the stdlib feature
arg, not on the cross-features — but the build stamp *inside* it keys on the
cross-feature set, and a stamp mismatch re-runs cargo. The harness sets
`PERRY_GC_TRACE=1` only for workloads carrying `*_traced` runtime budgets, and
`PERRY_GC_TRACE` adds `perry-runtime/diagnostics`
(`optimized_libs/freshness.rs`). `loop_bound_semantics` is the one gated
workload with no `*_traced` budget, so it is the one compile in the job that
wants the other feature set. Measured with a debug compiler on an M-series
mini, from a cleared `target/perry-auto-*`:

| compile | duration | budget |
|---|---|---|
| first gated compile, diagnostics variant | 244.6 s | 300 s |
| `loop_bound_semantics`, non-diagnostics variant | 242.1 s | 300 s |
| every later flip between the two variants | ~4 s | 300 s |
| every other gated compile | 3–4 s | 300 s |

So warming one variant only moves the timeout from `h1_native_rep_equivalence`
to `loop_bound_semantics`, which is 242.1 s into a 300 s budget on a box faster
than a GitHub runner. The step warms both, then flips back to the variant 24 of
the 25 gated compiles want. It goes through the harness rather than a bare
`perry compile` precisely because the harness is what decides `PERRY_GC_TRACE`
per workload — a hand-rolled compile would warm a stamp no gate step wants.

The step asserts it warmed something, which is the substance of the change
rather than a flourish: `perry compile` **exits 0 when auto-optimize fails** —
the driver prints "using prebuilt libraries" and returns
`OptimizedLibs::empty()` — so a pre-warm that quietly warms nothing is worse
than none at all, leaving the gates to fail exactly as before under a green
"Pre-warm" step. Two assertions, because neither covers the other:

* A stamped `target/perry-auto-*/` holding a non-empty `libperry_runtime.a`.
  Both halves are load-bearing: cargo creates the directory and a zero-byte
  `.perry-auto-build.lock` *before* building anything into it, while
  `.perry-auto-build.stamp` is written only past the `status.success()` check
  and is what the next compile consults for freshness.
* The third pre-warm's own compile duration, which must be under half a gate
  step's budget. That compile deliberately flips the stamp, and a flip is cheap
  only if *both* variants are in cargo's cache — so it fails a job warmed for
  one variant, which the artifact check above cannot see.

Both were checked against the failures they exist for, not merely exercised.
With the pre-warm neutered (`PERRY_NO_AUTO_OPTIMIZE=1`, runtime served from a
pre-existing archive so the compile still succeeds) the harness exits 0 and the
step goes red on the first assertion, naming the missing directory; its five
states — no `target/`, a directory mid-build, a stamp with a zero-byte archive,
an archive with no stamp, and fully warm — were each checked individually.

Verified afterwards that the linking compile still runs *with* auto-optimize,
i.e. that the #10782 exclusion is intact: `gc_trace_unavailable` is `False` on
all 23 suite workloads, which is the executed binary's own report that
`perry-runtime/diagnostics` was compiled in, and the harness fails the workload
when it is not. A suppressed linking compile would flip it to `True` rather
than pass vacuously.

Not done here, and worth its own issue: `--no-link` could decline to
auto-optimize outright, a two-line guard at `run_pipeline.rs:6169`, which fixes
the class rather than this instance.
