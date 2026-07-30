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
