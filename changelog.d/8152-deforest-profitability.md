**Deforestation now requires a consumer-fuse call site (#8104).**
`perry-transform/src/deforest` had no profitability term: `detect.rs` matched
`const out = []; …push…; return out` and `run` rewrote every producer it found,
at every call site. Only one of the three call-site shapes removes work — the
consumer fuse (`let X = f(args); for (j) outer.push(X[j])` → `outer = f(args,
outer)`) deletes a copy loop and leaves one array where there were two. The
other two are pure cost, and the value-binding rewrite is actively harmful:
`const arr = build(n)` becomes `let arr = []; arr = build(n, arr)`, which
allocates exactly as many arrays as before (the producer stops allocating one,
the caller starts) and costs the caller's binding its write-once property —
taking `Ptr<NumArray>`, the element-shape versioned loop clone, and every other
representation fact keyed on a stable local with it.

Sweeping `PERRY_DEFOREST_DEBUG=1` over `benchmarks/suite`,
`benchmarks/app-patterns/kernels` and the beat-scriptc sweep corpus finds
exactly three programs whose emitted object changes, and **none of them has a
fuse site** — all three were paying the cost for nothing. Measured on macOS
arm64, `--profile perry-dev` compiler and `libperry_{runtime,stdlib}.a`,
`PERRY_RUNTIME_DIR` pinned, `PERRY_NO_AUTO_OPTIMIZE=1`, `/usr/bin/time -l`,
medians of 5 interleaved, byte-identical output on both arms:

| program | instructions before | after | peak RSS before | after |
|---|---:|---:|---:|---:|
| `bench_numeric_array_numeric` | 232,687,808,231 | 7,681,530,021 (**-96.7%**) | 44,057,080 | 21,725,616 (**-50.7%**) |
| `shapes` (sweep corpus) | 1,574,232,277 | 1,507,089,862 (-4.3%) | 29,491,656 | 29,491,656 (0.0%) |
| `batch` (app-patterns) | 1,793,403,047 | 1,793,372,968 (±0.0%) | 20,169,184 | 20,169,184 (0.0%) |

`bench_numeric_array_numeric` drops from 4071 ms to 126 ms for the same
`checksum:6500625`. Both objectives move the same way on every row.

`producers_with_fuse_sites` shares the matcher with the rewrite
(`match_consumer_fuse_pattern`, factored out of `try_consumer_fuse_pattern`) so
the profitability question and the rewrite cannot disagree about what a fuse
site is, and mirrors `rewrite_call_sites_in_stmts_with_local_pass`'s descent so
they cannot disagree about which positions are reachable. A producer with one
fuse site anywhere keeps the rewrite at ALL of its sites — the signature gained
a parameter, so every call must pass one (#5136's arity-mismatch SIGSEGV).
The recursive fuse inside a producer's own body counts, which is what keeps
ABC451D deforested: `test-files/test_deforest_growth_forwarding.ts` still
reports `[deforest] producer fn_id=1 name=tree` under the gate and both
`test_deforest_*.ts` files stay parity-clean.

Six existing unit tests needed a fuse site added to their fixtures, which is
itself the finding: the whole pre-existing suite was built on
value-binding-only modules — the shape measured above as a 30x pessimization.
Each keeps its original subject.
