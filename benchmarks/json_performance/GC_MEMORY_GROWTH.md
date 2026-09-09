# Large JSON churn: memory evidence for the GC work

These measurements use the immutable corrected R2 worker from PR #10034,
not either new parser optimization. Main's large-container and collection
policies are unchanged in that correction. All measurements below are
whole-process peak RSS on the same M1/8 GiB host and include eight live inputs.
Each process has zero warmup and a different fixed count of rotating parses.
The last output remains live. Diagnostics are enabled, so these are not CPU
acceptance measurements or a proposal to ship the full-GC diagnostic mode.

| Fixture | Parses | Default peak MiB | Full-GC-mode peak MiB |
|---|---:|---:|---:|
| wide_1m | 1 | 29.83 | 29.83 |
| wide_1m | 8 | 66.33 | 62.84 |
| wide_1m | 16 | 85.61 | 64.22 |
| wide_1m | 32 | 133.84 | 65.19 |
| records_object_20m | 1 | 200.16 | 200.14 |
| records_object_20m | 8 | 451.72 | 341.94 |
| records_object_20m | 16 | 730.72 | 350.92 |
| records_object_20m | 32 | 1333.42 | 369.16 |

[Default raw evidence](results/quiet-r2-memory-growth/README.md),
[full-GC diagnostic evidence](results/quiet-r2-memory-growth-fullgc/README.md).
Both runs pass quiet admission and all process/finite-checksum/lifetime guards.
The eight-member serialized-output oracle was checked in the prior full matrix;
these diagnostic runs do not repeat it. Each row is a separate process.

## What is observed

The 32-parse large-object default run performs seven in-place minors, five of
them untraced. The traced young-survival measurements are 995 and 999 permille.
At the final trigger, old_in_use and old_reclaimable are both 1,026,443,760 bytes;
old_baseline is 896,172,312 bytes, old_band 1,792,344,624 bytes, retaining=true,
and old_pending=false. The incremental cycle starts and completions are zero.

There are four synchronous full collections in the default process, all during
input loading (the one-parse process has the same four). The full-GC-mode
32-parse process has fourteen full collections and returns to about 173 MB
arena occupancy at its last recorded collection. This supports deferred
reclamation rather than a requirement to retain all the parsed graphs.
The diagnostic counters are snapshots at collection sites, not exact live-byte
measurements at every RSS sample.

## Source-backed mechanism to verify in a targeted allocation experiment

The source explicitly documents this retention mechanism in
`gc/types.rs`: pointer-bearing allocations above 128 KiB are born tenured;
minor collections follow their remembered old-to-young edges even if the old
container itself is unreachable. Large parsed arrays and wide objects cross
that threshold. A dead large container can therefore keep a much larger child
graph alive until an old-generation collection establishes the parent's death.

`gc/policy.rs::credit_promoted_bytes_to_old_baseline` credits every promotion,
including untraced promotion. `note_copying_minor_young_survival` widens and
re-baselines major pacing when young survival is high. The observed retention,
promotion and growing baseline are consistent with a feedback loop: dead old
containers inflate young survival, that survival delays old reclamation, and
more graphs accumulate. A targeted placement/parent-liveness experiment is
still needed to attribute the entire curve to that mechanism.

Do not simply raise the nursery large-object cutoff: copying.rs currently has
a 1 MiB MAX_YOUNG_MOVE_BYTES ceiling, and large JSON array backings can exceed it.
Do not remove promotion credit or force a full collection per parse: existing
retained-heap tests document the resulting CPU regressions. Acceptance must
include both discarded and deliberately retained large outputs, CPU and RSS,
default GC, active marking, moving/protected GC and full-GC correctness.

## A JSON-local construction opportunity

`json/construction_array.rs::grow` completes layout and publishes remembered
edges for an old prefix before copying it into the next private array. The
prefix is then abandoned. Its comment calls it unreachable, but the old-to-young
edges still retain children through minors until an old sweep occurs. Because
this array is unpublished and exclusive to a suppressed construction, its
retired slots can potentially be cleared after copying, or its intermediate
backings can stay in native scratch storage until a final managed array is
allocated. Both require correctness and performance measurements. Neither alone
solves the dead final large-container problem above.

No GC policy, allocation placement or construction behavior has been changed
by this investigation. The parser scanner candidate remains independent.

## Earlier prefix-retirement experiment

[The preserved prefix-retirement trial](results/json-prefix-retirement/README.md)
already tested clearing abandoned construction buffers and publishing only the
final array. It reduced remembered-slot reads 62.9% and total slot reads 9.0%,
while promotion and RSS remained essentially unchanged: final dead old arrays
still held the children. That candidate was rejected after a 1.55% numeric-parse
slowdown in all seven paired groups. It used the older 16-codegen-unit comparison
profile, so it is not a measurement of the current default release profile.
Revisiting it could target duplicate tracing work, but it is not a demonstrated
solution to the final-container memory growth documented above.
