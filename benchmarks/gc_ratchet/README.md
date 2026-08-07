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

Twelve probes in `probes/`, each a deterministic TypeScript workload that
drives a different part of the collector: nursery churn with a zero live set,
survivor aging and promotion, old-to-young stores and the remembered set, dead
objects left under a deep stack high-water mark, closure environments, heap
strings, array element-storage growth, Map/Set side tables, try/catch rooting,
receiver stores across allocation points, collection under stack depth, and a
~100 MB live set held across many collections (the shape that catches
survivor-space saturation — every other probe's live set is small).

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

### `PERRY_CONSERVATIVE_STACK_SCAN=full` is this gate's sensitivity arm — keep it

CLAUDE.md's kill-policy says an unexercised mode gets deleted, and `=full` was a
clean candidate: it failed **134 of 1574** `perry-runtime` tests, a shipped
escape hatch nobody had verified (#7148). It was **kept** rather than deleted
for the reason this section documents — it is the only end-to-end proof that
`gc-ratchet` can fail. Deleting the mode would delete a gate's proof, which is
the same failure the kill-policy exists to prevent, one level up.

It is verified instead. The 134 failures were not soundness failures: a
collector test's central assertion is *"this object should have been
collected"*, and an ambient conservative scan retains whatever the native stack
looks like a pointer to, so `=full` broke exactly the assertions the suite
exists to make. Since #7147 the test build has one declared scan mode and the
isolation guards are its authority, so as of #7148 a pinned per-thread override
beats an env request for `Full` **in the test build only** — the env var may
make the scan less aggressive than a test declared, never more. Production
binaries pin no override, so the arm above is unchanged and still forces the
scan.

The `heap_used_bytes` column above is also the quantitative case behind #7148:
every `force_full_scan()` that fires at runtime is a collection that runs no
copying minor, so the four *automatic* fallback sites were each worth this much
RSS whenever they were reached. They are now counted
(`[gc-scan-fallback] site=… automatic=…` under `PERRY_GC_DIAG`), so a probe run
shows directly whether any of them fired.

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

## Taking one cell out of the gating family (`probe_overrides`)

`tolerances.json` is keyed per metric per profile. That is the right
granularity for a *band*, which expresses a machine class's noise floor. It is
the wrong granularity for "can this metric carry a gate at all on this
workload", which is a property of the workload — and the two were conflated
until #7554.

The symptom: `12_large_live_set` retention stopped being bit-identical, the
pinned artifact recorded a non-zero spread, and the assertion that refuses to
gate a metric on a spread it cannot support fired — **in the CI step that runs
before the measurement step**, so all twelve probes stopped running on every
branch for two days. The prescribed fix, "take this metric out of the gating
family for this probe", could not be expressed: the only lever turned
`heap_used_bytes` gating off for all twelve.

So `tolerances.json` has a `probe_overrides` section:

```json
"probe_overrides": {
  "12_large_live_set": {
    "heap_used_bytes": {
      "gating": false,
      "rationale": "NOT GATED ON THIS PROBE. …why…",
      "evidence": {
        "observed_runs": 21,
        "observed_spread": 4536,
        "measured_on": "…host, commit, build…",
        "issue": "https://github.com/PerryTS/perry/issues/7554"
      }
    }
  }
}
```

Deliberate properties, each of them a refusal:

- **It may only set `gating` to `false`.** An override exists to remove a cell
  from a gating family. Putting one back is the profile's job, where a reader
  looking for what is gated will find it.
- **It never touches the band.** An excluded cell is still measured, still
  compared, and still printed — a breach shows as `drift (informational)`
  rather than vanishing. `check` also prints every override with its reason
  under the table, so a `no` in the Gating column can be explained without
  opening another file.
- **The evidence is checked, not merely stored.** At least 21 runs — the same
  number every band in the file is justified by — and a spread that is
  actually non-zero. You cannot exclude a metric you have not shown is
  ungateable.
- **An override that matches no probe fails**, the same rule
  `scripts/gc_root_dominance_allowlist.json` carries. Fixing the
  non-determinism means deleting the entry, not leaving it to outlive its
  reason.
- **Overriding every probe for a metric fails.** Assembled one cell at a time,
  that is a profile-level `"gating": false` with nowhere to read the reason.

The bit-identity rule itself now lives in `validate_artifact`, so an artifact
carrying a non-deterministic gating cell cannot be *pinned*. Before #7554 the
rule existed only in `tests/test_gc_ratchet.py`, which is why a bad pin could
be committed and only wedge CI afterwards.

`heap_total_bytes`, `minor_cycles`, `copied_objects`, `promoted_bytes` and
`freed_bytes` on `12_large_live_set` all remain gating, so a real over-retention
regression on that probe still goes red.

### What that probe's non-determinism actually is

Worth knowing, because it is a property of the *metric* rather than of the
collector's steady state. Every probe reads `process.memoryUsage()` after an
explicit `gc()`, and an explicit `gc()` runs a full mark-sweep with a **forced
conservative stack scan** — `PERRY_GC_DIAG` prints `[gc-scan-fallback]
site=manual_collect automatic=false` on every run. A conservative scan retains
whatever the native stack happens to look like a pointer to, and the stack
residue at that moment differs between runs.

Diffing two full traces that disagree shows this directly: the minors, the
tenuring decisions, the step cycles and every copy/promote counter match
exactly, and the only difference is in the *last* collection's `freed_bytes`.
And with `PERRY_CONSERVATIVE_STACK_SCAN=off` the probe reports **51,668,688
bytes on 8 consecutive runs, bit-identical**.

Two things follow. The variance is entirely false roots, so it is bounded by
how much a handful of stale stack words can pin — a few kilobytes here. And
the conservative scan is retaining **8.28 MB, 16% of this probe's reported
retention**, systematically. The eleven small probes stay bit-identical because
their live sets are one to two orders of magnitude smaller, so a stale stack
word is far less likely to alias a plausible heap address at all.

### The measurement must show the collector ran

`check` fails a probe whose current run reports `minor_cycles == 0` or
`copied_objects == 0` where the baseline reports more, rather than leaving that
to the tolerance arithmetic. The arithmetic could not catch it: six probes pin
`minor_cycles` at 1 and the allowance floor is also 1, so a collapse from 1 to 0
is `delta == -allowance` and scored `ok`. The largest regression this ratchet
exists to catch — a collector that stops running copying minors — was being
reported as passing.

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

## What `heap_used_bytes` actually contains (#7559)

**A retention row is not evidence of a collector regression until it has been
classified.** Two properties of the measurement point make this metric move for
reasons that have nothing to do with what the collector retained:

1. **The measurement forces the conservative native-stack scan.** Every probe
   reads `process.memoryUsage()` immediately after an explicit `gc()`, and an
   explicit `gc()` is the one site in Perry that forces that scan
   (`ManualGcScanGuard`, #4977 — the production default is `Auto`, which skips
   it). So the reading is taken under a root set nothing else in the language
   uses, and it includes whatever the native stack happened to look like a heap
   pointer to at that instant.
2. **`js_arena_stats` sums block *offsets*, not live bytes.** A block's
   bump pointer never moves backwards, and a block holding one marked object
   cannot be reset — so a single stale stack word costs a whole **1 MiB nursery
   block**, an amplification of roughly 26,000x. (This is the nursery's version
   of the old-generation accounting bug #7437/#7443 fixed by subtracting swept
   holes.)

Measured across the 74 commits between the 2026-08-05 pin (`5e236e6e2`) and
v0.5.1321, both endpoints built identically and both reproducing the pinned
artifact byte-for-byte on all twelve probes:

| | probes moved | direction |
|---|---|---|
| `heap_used_bytes` (what the gate compares) | **5 of 12**, always by whole blocks | 3 down, 2 up |
| retention with the scan off (what the collector kept) | **2 of 12** | both **down** |

`05_closure_capture`'s `+16.44%` breach is the extreme case: its precise
retention was **5,329,880 bytes at both endpoints, to the byte**, while its
false-root residue went from one 1 MiB block to two. `02_survivor_promotion`'s
`+2.77%` is the same shape one survivor block down (256 KiB), and its precise
retention *fell* by 1,600 bytes. Neither is a collector regression.

The probe's own live set is not what is being reported either: sweeping
`05_closure_capture`'s `BATCHES` from 690 to 710 — a workload whose live set is
approximately zero at the measurement point for every value — moves
`heap_used_bytes` between 6,501,264 and 7,426,960 in a 1 MiB sawtooth, because
what is left over is the un-reset tail blocks' bump pointers.

### `classify` — split residue from retention in one command

```bash
PERRY_RUNTIME_DIR=$PWD/target/release PERRY_NO_AUTO_OPTIMIZE=1 \
python3 benchmarks/gc_ratchet/gc_ratchet.py classify --perry target/release/perry
```

It runs every probe twice — once as the gate does, once with
`PERRY_CONSERVATIVE_STACK_SCAN=off` — and prints the split, plus the census of
which conservative-scan sites actually fired. It refuses to tabulate a probe
whose *output* changes when the scan is disabled (the scan was load-bearing for
that probe's correctness, so its precise number is not evidence), and it refuses
to report a precise reading that is not bit-identical across repeats. The
conservative reading is allowed to vary and its spread is reported instead —
that spread on `12_large_live_set` is why #7554 had to stop gating the cell.

A row whose `excess` moved and whose `precise` did not is a false-root artifact.
A row whose `precise` moved is a real retention change and the rest of this
document applies to it.

## When the gate goes red

1. **Read the table.** The failing rows name the probe and the metric. Retention
   up means something is being kept alive that used to be collected.
   `copied_objects` down means objects that used to be evacuated no longer are.
   `freed_bytes` down means the same allocation sequence reclaimed less.
2. **Classify a retention row before believing it** — `gc_ratchet.py classify`,
   above. This step is not optional: #7559 was a `+16.44%` retention breach with
   every collector counter at `+0.00%`, and the answer was that nothing was
   retained that had not been retained before.
3. **Reproduce locally** with the ad-hoc commands above. Retention and GC
   counters do not need a quiet box — they are load-independent.
4. **Fix it, or accept it.** If the shift is intentional and reviewed, re-pin:

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
| `gc_ratchet.py` | measure / classify / assemble / check / validate |
| `tolerances.json` | every band, with the variance it was derived from |
| `baseline/gc-ratchet-v1.json` | the pinned artifact |
| `run_gc_ratchet_baseline.sh` | quiet-host driver (`--check` / `--pin`) |
| `../../tests/test_gc_ratchet.py` | proves the gate fails on each failure mode |
| `../../.github/workflows/gc-ratchet.yml` | CI wiring |
