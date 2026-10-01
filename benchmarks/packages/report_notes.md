## Notes, known issues and what was not measured

**This is the post-fixing-phase re-run** (2026-09-30) at `origin/main` **`9e29f59d43`** — the merge of #11645
(survival-aware nursery pacing + #11612 regex memory). The pre-fix baseline is the Phase-1 run at `36420d2e56`
(this file's previous version, commit `24a5262bd6`). The before/after comparison, the re-ranking and the attribution
of the top offenders are in [`RERUN-2026-09-30.md`](RERUN-2026-09-30.md).

**Build.** Perry arm = `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` (the workspace
`[profile.release]`), built **separately on each host from the same commit** `9e29f59d43`, then plain
`perry compile <wl>.ts -o <bin>` with **no flags** (auto-optimize ON; each feature set rebuilds its own
`libperry_{runtime,stdlib}.a` under `target/perry-auto-*`, the † compiles). No `PERRY_GC_*`, `PERRY_WRITE_BARRIERS`,
debug or `PERRY_NO_AUTO_OPTIMIZE` variable was set; `PERRY_BUILD_COMMIT` was set to the built commit and
`PERRY_RUNTIME_DIR` was unset. To fit the Linux host's disk, workloads were compiled one at a time and each
`perry-auto-*` dir's intermediate `deps/`/`build/` trees were deleted after its archives were stamped (the cached
archives + stamp are all the freshness check reads, so later compiles still reused them) — compile times are
therefore per-workload and † marks the first compile of each feature set, as before. Binaries are stripped
(default); the liveness pass recompiled the same sources with `PERRY_KEEP_SYMBOLS=1` (`compile-linux-symbols.json`).

**Hosts.** Instructions (+ a Linux RSS sample) on `perrymaster` (x86-64); its wall-clock is not trusted, so none is
reported from it. Wall-clock, cold start and the RSS table on the quiet M1 bench mini (Node 26.5.1, Bun 1.3.14 —
the same pins as the baseline). Both runs took the shared mkdir mutex `/tmp/perry-bench-lock.d`. **Both hosts were
quiet this time**: max 1-minute load during any measured sample was 2.94 on perrymaster (16 CPUs) and 2.52 on the
mini (8 CPUs), and no workload was load-flagged. The Phase-1 baseline's instruction host ran at load ~30–53 and every
workload there was load-flagged; instruction counts are far less load-sensitive than wall time, but treat
small (< ~5 %) before/after instruction deltas as noise. The mini has no Postgres / MySQL / MongoDB / Redis, so the
database workloads have **instruction counts only** (as in the baseline).

**Controls.** `control/bare_loop` 26 instr/iter (Node 58), unchanged. `control/prop_read` 130 (Node 55) — was 119;
see RERUN-2026-09-30.md. **No control is subtracted from any workload.**

**Perry failures on `9e29f59d43`** (each reproduced outside the harness, deterministic 3/3):

| workload | symptom | issue |
|---|---|---|
| `node-forge/rsa_sign` | compiles now (#11450 fixed codegen) but throws `TypeError: Cannot read properties of undefined (reading 'data')` on the first sign; Node prints the checksum | none filed yet (listed for triage) |
| `fastify/inject` | `ERROR Cannot read properties of undefined (reading 'once')` — a new symptom; the baseline's `setHeader is not a function` (#10454) is fixed | none filed yet (listed for triage) |

**Fixed since the baseline** (failed at `36420d2e56`, correct now): `node-forge/sha256`, `/hmac`, `/aes_cbc` (#11450),
`jsonwebtoken/rs256` (#11447), `node-cron/match` (#11446), `pg/select`, `pg/insert_batch` (#11459),
`rate-limiter-flexible/consume` (#11449 via #11457), axios binary portability (#11448).

**`mongodb/insert_find` now runs correctly** (3/3 instruction reps plus the correctness run, output identical to Node)
although the manifest still marks it `known_hang` (#11460, still open). The main `run` skipped it per the manifest;
it was then run separately at the same commit with `--include-known-hangs` and merged into `instr.json`
(`note_rerun` field). Whether #11460 can be closed is for its owner to confirm; the manifest entry is left as is.

**Bun failures** (not Perry's): `mongodb/*` — Bun 1.3.14 throws `node:v8 isBuildingSnapshot is not yet implemented`.

**`exponential-backoff/retry` wall ratio is not a compute comparison.** It retries with a 0 ms delay: Node clamps
`setTimeout(0)` to ≥ 1 ms, Perry fires it immediately. Use its instruction ratio.

**Two-N caveats.** Node/Bun instruction counts include their JIT and GC threads; Perry's include its GC. The method
assumes cost is linear between n1 and n2.

**Not done:** a full Phase-3 `--callgraph` sweep (only the top offenders were re-profiled — see RERUN-2026-09-30.md;
`PROFILE.md` still describes the `36420d2e56` attribution); a comparison against the removed native bindings (owner
decision); a CI/nightly job.
