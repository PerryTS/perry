# Bounded collection deferral after JSON construction

This experiment builds on the construction batch at `fb69a7059` and changes
GC scheduling with the user's explicit approval. The earlier batch experiment's
unchanged-policy claim applies to that parent, not to this branch.

After successful suppressed construction, the JSON entry completes object
metadata, roots the result, restores collection scheduling, and records
allocation pressure without running a collection in the completion hook. Entry
checks can still service previous debt, and pressure can still collect in later
caller operations. This is deferred same-thread collection, not concurrent GC.

## Admission and bounds

The final candidate measures occupied arena space across construction for inputs
at least 256 KiB. It offers ordinary nursery pressure a grace period only if that
construction fits the allowance: at most 8 MiB, or 1/32 of the configured heap
budget when smaller. Tiny leaf parses keep their existing scheduling path.

The grace period expires after two handled precise safepoints, or when arena
allocation high-water space grows by the allowance, whichever happens first. Repeated parses
cannot renew either bound. A completed collection resets the allowance. The
scheduler keeps its existing poll armed so a subsequent reachable poll can
service the debt even without further allocation.

These are scheduling bounds, not a hard RSS cap. A suppressed parse can exceed
the byte allowance by its construction size, and the existing allocation-point
fallback has its own bounded slack when no precise poll is reached. Native
buffers, live objects, allocator residency and verification also contribute to
process RSS. A larger completed construction gets no new grace period.

Old-generation pressure, explicit collection and seed-selected stress
collections bypass the grace period. OS memory pressure cancels it, including
when the warning arrives in an unsafe window. An already active budgeted cycle
continues through its existing protocol. The new state holds byte counts and
flags, with no managed pointers, extra roots or per-object allocation registry.
Native tape buffers remain charged to old-generation pressure, which bypasses
the allowance. GC suppression ends with construction; ordinary write barriers
remain active while the scheduler delays collection.

This changes scheduling only. It retains the parent's construction batch,
object layouts, write barriers, tracing, moving-root rewrites and stringify
callback behavior. Stringify receives no independent grace period; a
parse-then-stringify workload may use the parse's remaining allowance.

## Measurement

The original 38-row worker remains unchanged and charges the whole loop,
including collection before or after individual calls. It retains its newest
output in `last`. `lifetime-worker.ts` adds a separate paired probe for discarded,
latest-only and retained outputs, parse and stringify API durations, whole-loop
CPU, final explicit cleanup CPU, and process RSS. Its per-call timer overhead and
explicit cleanup make it a different workload; compare it only with its matched
parent binary. Retained outputs are read and checked after cleanup.

The first, broader implementation (`a58ebc72f`) passed 3,277 runtime tests and a
qualified four-engine matrix. Wide-object parse CPU fell about 25%, but empty
and tiny parse scheduling gained overhead and the 8 MiB record parse regressed
about 28%. Its full results are preserved. The narrower admission above is a
separate candidate, measured independently rather than mixed into that run.

This is not all-row parity or no-regression acceptance. The inherited semantic
gaps and earlier CPU/RSS requirements remain open.

Final measured source: `0327b9460a749592deb354d72ebad29bf4ae4bba`.
See [the results and validation](results/gc-deferral/README.md), including
[all 38 CPU rows](results/gc-deferral/cpu-38.md) and
[seven paired repeats](results/gc-deferral/paired/table.md).

The [subsequent layout and direct-output investigation](results/json-layout-and-admission/README.md)
retains the same runtime. It demonstrates linker-placement sensitivity and a
25-versus-35 collection-count reproducer for the large round-trip regression;
both attempted stringify changes were rejected.

The [root investigation](results/json-root-retention/README.md) identifies the
first retention difference in unchanged binaries: the checkpoint conservatively
retains the previous call's large array, which delays later collections.
Debugger-only interventions distinguish stale stack roots from remembered-set
retention through dead old containers; they do not qualify a new GC policy.

The [array-prefix experiment](results/json-prefix-retirement/README.md) reduces
duplicate remembered-set scanning and improves large round-trip CPU by about
1–2%, but was reverted after a seven-pair numeric-parse regression. Runtime
source remains the same checkpoint; shipping-profile validation is next.

The [default-release comparison](results/json-shipping-profile/README.md) now
validates both arms with settings matching `dist`, with fresh Node/Bun results
for [all 38 rows](results/json-shipping-profile/measurements/recheck/all-38.md).
The prefix candidate still fails acceptance: a +0.45% record-object parse
regression repeats, and large retained parse peak RSS increases. The reference
matches or beats both competitors' CPU medians in 15 of 38 rows in that run.

The [string-counting investigation](results/json-string-counting/README.md)
preserves an allocation-free ARM UTF-16 counter on the separate experimental
branch `codex/json-utf16-count-1520`. It improves Unicode parse CPU, with all
correctness and moving-GC checks passing, but has not met no-regression
acceptance. A same-binary control exposes invocation-path sensitivity in the
large lifetime benchmark; the report includes corrected comparisons through
one fixed executable path. The original GC branch remains at `ae1f89611`.
