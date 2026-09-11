# Cached lazy reads — R22

R22 removes runtime rooting from allocation-free sparse-cache hits. In the first quiet access window, repeated reads used about 49% less CPU, 16 KB field-reading iterations used 34.17% less, and 16 KB sequential reads used 24.87% less. Peak RSS was essentially unchanged. The 1 MB random-read row used 1.16% more CPU with separated sample ranges, so this source remains unmerged and is not qualified as a regression-free improvement.

The next step is the already-materialized array path: R22 still enters the old rooted accessor there, after the new dispatch check. The follow-up should return existing dense elements without rooting, while preserving the rooted fallback for holes, descriptors and other cases that can invoke user code. The measured regression does not establish which instruction caused it.

Previously accepted JSON changes are on main through PR #10037 (the merge train for closed PR #10036). R21 is a separate, parked experiment; it is not included here. This candidate starts from main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531), with measured source `8b7ffae96e733c96b31aa41d064902bc4f9a50ce` on `codex/json-lazy-cached-read-r22`.

**Method and limits.** This window measures consumption after one parse: parsing is outside the timed interval. Each row has 1,000,000 iterations, zero warmup, seven fresh-process trials per engine, randomized interleaving, and a Node checksum. A `fields` iteration reads `id`, `name.length` and `active`; it is not a single property access. The host is the quiet M1/8 GiB mini, Node 26.5.1 and Bun 1.3.14. All 336 timed checksums, sample vectors and medians were independently recomputed from the archived logs. The quiet gate passed; the completed window was archived before any later remote operation.

R22 has not yet rerun the original 38 parse/stringify rows, consumption scans/roundtrips, changing-input parsing, retained-memory workloads or stringify-option performance. Passing option behavior is separate from measuring option speed. No all-row or Node/Bun parity claim is made.

**Median CPU, microseconds per iteration.** Negative delta means less CPU than main. “Separated” means the two observed sample ranges do not overlap; it is not a population confidence interval.

| Input | Access | Main | R22 | Node | Bun | R22 vs main | Ranges |
|---|---|---:|---:|---:|---:|---:|---|
| records_array_16k | repeat | 0.018567 | 0.009397 | 0.002727 | 0.004527 | -49.39% | Overlap |
| records_array_16k | random | 0.040327 | 0.040094 | 0.006664 | 0.009290 | -0.58% | Overlap |
| records_array_16k | fields | 0.084464 | 0.055600 | 0.005334 | 0.009227 | -34.17% | Separated gain |
| records_array_16k | sequential | 0.035098 | 0.026368 | 0.003882 | 0.005735 | -24.87% | Separated gain |
| records_array_1m | repeat | 0.018545 | 0.009412 | 0.002779 | 0.004546 | -49.25% | Separated gain |
| records_array_1m | random | 0.048840 | 0.049406 | 0.010519 | 0.010751 | +1.16% | Separated regression |
| records_array_1m | fields | 0.107952 | 0.107491 | 0.010798 | 0.014341 | -0.43% | Separated gain |
| records_array_1m | sequential | 0.040453 | 0.040463 | 0.008173 | 0.007497 | +0.02% | Overlap |
| records_array_20m | repeat | 0.005647 | 0.005637 | 0.003042 | 0.004803 | -0.18% | Overlap |
| records_array_20m | random | 0.025894 | 0.025646 | 0.008638 | 0.010031 | -0.96% | Separated gain |
| records_array_20m | fields | 0.027165 | 0.027269 | 0.011666 | 0.014581 | +0.38% | Overlap |
| records_array_20m | sequential | 0.015618 | 0.015564 | 0.006777 | 0.008227 | -0.35% | Overlap |

The 16 KB field row remains 10.42× Node and 6.03× Bun despite its improvement. The 1 MB field row is still about 9.95× Node. The remaining work includes generic array/property access, not only JSON tokenization.

**Median peak process RSS, MiB.** These are process peaks for this access workload, not a retained-heap or leak measurement.

| Input | Access | Main | R22 | Node | Bun | R22 − main |
|---|---|---:|---:|---:|---:|---:|
| records_array_16k | repeat | 13.047 | 13.047 | 57.828 | 34.812 | +0.000 |
| records_array_16k | random | 13.422 | 13.422 | 57.906 | 35.406 | +0.000 |
| records_array_16k | fields | 13.172 | 13.172 | 57.984 | 35.984 | +0.000 |
| records_array_16k | sequential | 13.188 | 13.172 | 57.859 | 35.234 | -0.016 |
| records_array_1m | repeat | 17.578 | 17.578 | 63.719 | 37.938 | +0.000 |
| records_array_1m | random | 18.484 | 18.469 | 66.625 | 38.906 | -0.016 |
| records_array_1m | fields | 18.266 | 18.266 | 66.781 | 40.312 | +0.000 |
| records_array_1m | sequential | 18.281 | 18.281 | 66.609 | 39.219 | +0.000 |
| records_array_20m | repeat | 96.906 | 96.922 | 194.734 | 93.953 | +0.016 |
| records_array_20m | random | 96.922 | 96.922 | 194.859 | 94.750 | +0.000 |
| records_array_20m | fields | 96.922 | 96.922 | 195.062 | 95.344 | +0.000 |
| records_array_20m | sequential | 96.922 | 96.938 | 194.906 | 94.594 | +0.016 |

**Mechanism.** The inline wrapper checks the live lazy header, materialization state, bounds and bitmap before returning the current cached slot. It allocates nothing, invokes no user code and has no safepoint. Cold construction and materialized/mutated arrays retain the existing rooted accessor. The emitted cache-hit block has 14 instructions and branches to the ordinary accessor epilogue without entering the extra 192-byte `lazy_get` frame or calling root-scope helpers. This count excludes the outer array receiver checks and epilogue. All four benchmark worker object files are byte-identical between main and R22. No GC policy, collector implementation, trigger threshold or cache admission rule changed.

**Main-only diagnostic profiles.** Three instrumented samples were taken separately from the access timings, with no overlapping remote work. The initial 16 KB sample had only 143 workload samples and substantial startup coverage; it is preserved as insufficient. A delayed, longer 16 KB recheck supplied 725 workload samples; the 1 MB sample supplied 754. Root-scope helpers account for 27.45% and 19.89% of those respective workload samples. Inclusive array-access shares are 41.10% and 67.37%; these overlap with the root-helper shares and must not be added. All profile checksums match Node. These shares are diagnostics, not isolated phase timers or measured gains.

**Validation.** The final clean source passed 293 JSON unit tests with `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`. Both frozen arms passed 46 Node behavior checks and 14 stringify-option checks. The new cached-read fixture covers identity, bitmap boundaries, out-of-bounds reads, child mutation, replacement, growth, shrinking, holes and retained values. Under scheduled GC it recorded 1,175 protected retired sets and 63,847 moved objects in both auto and forced-tape modes; direct mode recorded 1,166 and 102,647. Existing forced-copy runtime tests also remain active.

Both arms produced 18 equivalent IR files after removing only the native ModuleID path comment. Shadow checks and the ordinary-worker/callback native subsets pass. The complete native fixture corpus retains 16 unsuppressed findings: 15 unrooted values and one stale use, including eight allocation findings exposed by the new fixture on main too. Coverage is 3,258 safepoints, 2,464 live bundles, 13,723 relocates and 13,633 safepoint/root pairs. This is baseline equivalence, not a clean full-native safety verdict.

Existing correctness gaps are preserved: the 24 lazy-stringify probes include six crashes and two noncanonical outputs; fractional spacing still differs from Node. A new, separately retained probe finds that defining an index getter after lazy-array growth/shrinking fails in auto/tape mode but passes with direct parsing and Node. Exact stdout/stderr outcomes match between main and R22. Inspection found that the Object.defineProperty array classifier declines GC_TYPE_LAZY_ARRAY even after materialization; R22 does not repair that route. The getter case was separated from the passing cache fixture, not counted as passing conformance.

Final local lint: 73 of 74 checks pass; the pre-existing public benchmark evidence freshness gate fails. The 2,000-line file cap passes. Compile-tier checks and two CI-only checks were skipped; this is not a full CI pass.

**Build provenance and repair history.** The exact production package set was `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. The final build started 2026-09-11T04:46:36.775003+00:00 and completed/froze in 507.069 seconds. All three artifact mtimes are after the recorded start. Main uses the independently rebuilt and hash-verified 1a9 artifacts from the prior investigation; main was reverified unchanged after this build.

| Artifact | R22 SHA-256 |
|---|---|
| perry | `067bbfa68dce9a351476081a66efe9ac2076389bb96b948c046f21debe8215b9` |
| libperry_runtime.a | `089dca2e0e09b8ecf6909fdd68a024036d51e28e5e4a1e39ac77d6409626d727` |
| libperry_stdlib.a | `093ea8ff15f2025b281adda4ce9ac488c8add2c0533fb4ab5cf2191e4beba2ad` |

Initial attempts caught a test API type mismatch, formatting, a generic test-helper name that confused the GC-holder call graph, and a raw-TLS integer counter requiring custody classification. The final test uses a uniquely named hook helper and a plain atomic integer; no inventory exemptions were added. One earlier production build completed but its artifact freeze was rejected when newly archived, untracked profile files made the tree appear dirty. A superseded test build was deliberately terminated before another production build. These failed/stopped attempts are retained separately; only the final clean-source build above supplies the measured candidate.

**Evidence.** The access window is `results/quiet-lazy-cached-read-r22-access/`; the initial and corrected profiles are `results/quiet-lazy-cached-read-r22-main-profiles/` and `results/quiet-lazy-cached-read-r22-main-profiles-recheck/`. The validation archive contains source/build hashes, commands, complete outputs, all static findings, failure history, raw disassembly and analyzers. Prepared scripts for unexecuted broader matrices are not evidence that those runs occurred.
