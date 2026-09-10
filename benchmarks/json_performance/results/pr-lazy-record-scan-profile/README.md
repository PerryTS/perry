# PR scan location profile

The immutable tape-depth R2 worker (the current PR runtime) ran the 1 MiB record
array scan with 8 warmups. The two-second local sample intentionally terminated
the worker afterwards. A concurrent local release build means these are location
diagnostics, not CPU acceptance timings. The full command and worker/source hashes
are in provenance.json; no worker source or benchmark fixture changed.

Of 1472 main-thread samples, 839 pass through js_array_get_f64 and 752 through
force_materialize_lazy into DirectParser's array parse. The tape builder has 402
self samples. The profile corrects the earlier incomplete scan hypothesis:
sequential scans trigger an adaptive full reparse, beyond the initial per-record
lazy materializations. The source's documented rationale is that its batched
DirectParser was ~1.8 times cheaper than its older element-wise tape producer;
blindly disabling the flip would restore that cost and is not an optimization.

A possible next investigation is compiler/runtime fusion of an indexed JSON
record's own property read. A pristine lazy record's scalar own field may be read
without creating its other properties. This is unimplemented and unmeasured.
Cached/modified records, materialized arrays, escaped ambiguous keys, missing
properties/prototype getters and non-scalar cases need normal fallback. Base/index
must be evaluated once, duplicate keys must preserve last-wins semantics, and
all collection-capable fallbacks require existing rooting guarantees.

Projection must not incorrectly count reads as materialized cache entries or
trigger the current batch flip. If sharing the walk cursor, reset the consecutive
materialization streak; do not add projection work to its materialization heuristic
without separate evidence. Compiler changes require freshly compiled baseline and
candidate workers, not the frozen runtime-only objects used for stringify trials.
