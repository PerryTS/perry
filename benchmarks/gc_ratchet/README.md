# GC ratchet

A pinned baseline of the current evacuating minor collector's observable
behaviour, plus a checker that fails when a change regresses against it.

It exists because the GC architecture campaign — replacing shadow-stack precise
roots with a conservative stack scan plus per-object pinning — is a large
deletion whose risks are exactly the things this measures: more retained
garbage, less evacuation, higher memory. The rule for that campaign is that the
gate decides, not argument.

## This is not the public benchmark baseline

Two artifacts in this repository sound alike and are unrelated. They get
confused; this section exists so they stop being confused.

| | public baseline | GC ratchet (this directory) |
|---|---|---|
| Artifact | `benchmarks/results/public-node-bun-v1.json` | `benchmarks/gc_ratchet/baseline/gc-ratchet-v1.json` |
| Produced by | `benchmarks/run_public_baseline.sh` | `benchmarks/gc_ratchet/run_gc_ratchet_baseline.sh` |
| Checked by | `benchmarks/ci_public_baseline_check.py` | `benchmarks/gc_ratchet/gc_ratchet.py check` |
| Compares | Perry vs pinned Node vs pinned Bun | Perry vs its own past self |
| Purpose | published performance evidence | internal regression ratchet |
| Gates | `lint` | `gc-ratchet` |
| Regenerated | on a release cadence, freshness-checked | only when a shift is deliberately accepted |
| Owner | maintainer | whoever lands a collector change |

Never regenerate one from the other. The quiet-host discipline in
`run_gc_ratchet_baseline.sh` is copied from `run_public_baseline.sh` on purpose,
because it is good discipline, not because the artifacts are related.

## What is measured

Eight probes in `probes/`, each a deterministic TypeScript workload that drives
a different part of the collector: nursery churn with a zero live set, survivor
aging and promotion, old-to-young stores and the remembered set, dead objects
left under a deep stack high-water mark, closure environments, heap strings,
array element-storage growth, and Map/Set side tables.

Every probe parks its allocations in a heap container before dropping them. This
is load-bearing. An earlier draft allocated into locals that never escaped, LLVM
scalar-replaced the objects away, and the probe ran in 10 ms with zero
collections and 600 retained bytes — a benchmark that measured nothing while
looking healthy. The harness now refuses to pin any probe that triggers no minor
collection.

Each probe writes two streams:

- **stdout** — `probe:` and `checksum:` lines only, diffed byte-for-byte against
  the Node version pinned in `.node-version`. Exit 0 is not correctness: a probe
  that quietly stops allocating still exits 0 and reports a beautifully small
  retained heap.
- **stderr** — `#gcmetric key=value` lines read from `process.memoryUsage()`
  after an explicit full `gc()`.

Four metric families, measured on the pinned quiet host over 3 independent
sessions x 7 repeats (21 runs per probe), plus 5 traced runs per probe:

| Family | Metrics | Observed spread | Gated in shared CI | Gated on pinned host |
|---|---|---|---|---|
| retention | `heap_used_bytes`, `heap_total_bytes` | **0.000%** | yes | yes |
| GC accounting | `minor_cycles`, `step_cycles`, `copied_objects`, `copied_bytes`, `promoted_objects`, `promoted_bytes`, `freed_bytes` | **0.000%** | yes | yes |
| memory | `rss_bytes`, `peak_rss_bytes` | <=0.41% | no | yes |
| timing | `wall_ms` | <=0.75% (medians) | **no** | yes |

The GC accounting family is parsed from `PERRY_GC_DIAG=1` output in a separate,
untimed pass; enabling the trace was verified not to change `heap_used_bytes`,
so the traced pass observes the same collector the untimed pass measures. The
harness takes two traced runs on every invocation and fails if they disagree —
that is the harness proving, each time it runs, that the counters it is about to
gate on really are deterministic.

Retention and GC accounting are semantic: they are a function of the allocation
sequence and collector policy, not of CPU speed, core count, or machine load.
That is why they can be gated on a shared CI runner and memory and time cannot.

## Why wall time is excluded from the shared-CI gate

On the pinned quiet host, wall time is stable enough to gate (0.75% spread on
medians of 7). On a GitHub-hosted runner it is not, and no band both survives
neighbour noise and catches a real GC slowdown. Rather than widen the band until
it can never fire, `wall_ms` is marked non-gating in the `shared_ci` profile and
gated in `pinned_host`.

The GC-work dimension is not lost by that choice: `minor_cycles`,
`copied_objects`, `copied_bytes` and `freed_bytes` measure how much work the
collector did without measuring how fast the machine was, and they are gated
everywhere.

`rss_bytes` and `peak_rss_bytes` are excluded from the shared-CI gate for a
different reason: a GitHub runner is a different machine class with a different
baseline RSS, so the comparison is not meaningful there at any band.

## The gate was validated against a real collector change

Unit tests prove the checker fails on injected JSON. That is necessary but not
sufficient — it says nothing about whether the *probes* are sensitive to the
thing the campaign will actually do. So the gate was also run end-to-end against
`PERRY_CONSERVATIVE_STACK_SCAN=full`, the runtime's existing escape hatch for
the legacy conservative stack scan, which is the mechanism the campaign is
adopting.

Control arm (unchanged collector) reproduced the baseline exactly on all eight
probes and exited 0. The conservative-scan arm exited 1 with 60 regression rows:

| Probe | `heap_used_bytes` baseline | with conservative scan | Δ | `minor_cycles` |
|---|---:|---:|---:|---:|
| `01_nursery_churn` | 807,000 | 3,742,800 | +364% | 14 → 0 |
| `02_survivor_promotion` | 5,022,864 | 25,242,760 | +403% | 10 → 0 |
| `03_cross_gen_writes` | 705,320 | 6,494,536 | +821% | 22 → 0 |
| `04_dead_after_deep_stack` | 1,000,728 | 9,187,088 | +818% | 104 → 0 |
| `05_closure_capture` | 1,040,208 | 7,336,208 | +605% | 26 → 0 |
| `06_string_retention` | 746,056 | 4,746,240 | +536% | 64 → 0 |
| `07_array_grow_evacuate` | 1,816,232 | 99,362,360 | +5371% | 80 → 0 |
| `08_map_set_sidetables` | 457,872 | 6,754,560 | +1375% | 84 → 0 |

Two things follow. The probes are sensitive to conservative scanning by orders
of magnitude, not by margin. And a collector that stops running copying minors
at all is reported as a regression on `minor_cycles` rather than as a harness
error — the "probe ran no collection" rule deliberately lives in
`validate_artifact` (you may not *pin* such a probe) and not in `measure`, so
that the largest possible regression is not misdiagnosed as "your probe is too
small".

## Cross-host evidence (why the shared-CI profile gates what it gates)

The baseline is captured on an 8-core M1 with 8 GB at load ~1.2. The first
`gc-ratchet` CI run executed on a **3-core virtualised M1 with 7 GB, macOS
14.8.7, at load 25.6** — a different machine class under heavy load. Comparing
that run's medians against the pinned baseline:

| Metric | Cross-host result |
|---|---|
| `heap_used_bytes`, `heap_total_bytes` | **bit-identical on all 8 probes** |
| `minor_cycles`, `step_cycles`, `promoted_objects`, `promoted_bytes`, `freed_bytes` | **bit-identical on all 8 probes** |
| `copied_objects`, `copied_bytes` | drift ≤0.06% on 6 of 8 probes, identical on the rest |
| `peak_rss_bytes` | within ±0.6% |
| `wall_ms` | **+54% to +60%** |

Three things are settled by this.

Retention really is load-independent: it reproduced byte-for-byte on a box under
load 25.6, which is the property the whole shared-CI gate rests on.

The evacuation counters are *nearly* host-invariant but not exactly so —
`copied_objects` and `copied_bytes` carry a sub-0.1% host-dependent component,
presumably because the scavenger's trigger interacts with allocation timing.
That is ~80× inside their 5% band, so they stay gating, but do not assume
bit-identity across hosts the way you can for retention.

Excluding `wall_ms` from the shared-CI gate was not caution, it was necessary.
The same binary is 54–60% slower on the runner. Any wall-time band tight enough
to catch a GC slowdown would have made every CI run red on day one.

## Direction

Retention, memory and timing regress only upward, so their bands are one-sided.
The evacuation counters are two-sided: they are a behavioural fingerprint of the
collector, not a score. A collector that suddenly copies fewer objects has
changed — plausibly because objects are now pinned — and must be re-pinned
deliberately rather than silently congratulated.

## Running it

Checking on the pinned quiet host, with memory and time gated:

```bash
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
PERRY_BIN=$PWD/target/release/perry \
PERRY_RUNTIME_DIR=$PWD/target/release \
  ./benchmarks/gc_ratchet/run_gc_ratchet_baseline.sh --check
```

The driver refuses to run unless the tree is clean, the host is on AC power,
there are at least 9 GB free, the pinned Node is present at the expected
version, and CPU-active has been at or below 25% for 60 consecutive seconds. It
requires `PERRY_BIN` and `PERRY_RUNTIME_DIR` to be named explicitly rather than
searching, because build outputs are invisible to `git status` and an implicit
search can pick up an archive from an unrelated worktree.

Ad-hoc, without the quiet-host gating (for a quick look, not for evidence):

```bash
python3 benchmarks/gc_ratchet/gc_ratchet.py measure \
  --perry target/release/perry --repeats 7 --output /tmp/current.json
python3 benchmarks/gc_ratchet/gc_ratchet.py check --current /tmp/current.json
```

## When the gate goes red

1. **Read the table.** The failing rows name the probe and the metric. Retention
   up means something is being kept alive that used to be collected.
   `copied_objects` down means objects that used to be evacuated no longer are.
   `freed_bytes` down means the same allocation sequence reclaimed less.
2. **Reproduce locally** with the ad-hoc commands above. Retention and GC
   counters do not need a quiet box — they are load-independent.
3. **Fix it, or accept it.** If the shift is intentional and reviewed, re-pin:

   ```bash
   PERRY_BIN=... PERRY_RUNTIME_DIR=... \
     ./benchmarks/gc_ratchet/run_gc_ratchet_baseline.sh --pin \
       --notes "why this shift is intentional and who reviewed it"
   ```

Re-pinning to make a red gate go green, without that reasoning written down in
`--notes` and in the PR, defeats the entire purpose of the ratchet. The artifact
records the commit, host, load average at capture, toolchain versions, and
SHA-256 of the `perry` binary and both runtime archives, so a re-pin is
auditable after the fact.

## Adding a probe

Adding or removing a probe changes the baseline's probe set, and the checker
fails on a set mismatch rather than silently ignoring the new one. So a new
probe requires a deliberate re-pin, which is the intended friction. The probe
must trigger at least one minor collection or the harness refuses to pin it.

## Files

| Path | Purpose |
|---|---|
| `probes/*.ts` | the workloads |
| `gc_ratchet.py` | measure / assemble / check / validate |
| `tolerances.json` | every band, with the variance it was derived from |
| `baseline/gc-ratchet-v1.json` | the pinned artifact |
| `run_gc_ratchet_baseline.sh` | quiet-host driver (`--check` / `--pin`) |
| `../../tests/test_gc_ratchet.py` | proves the gate fails on each failure mode |
| `../../.github/workflows/gc-ratchet.yml` | CI wiring |
