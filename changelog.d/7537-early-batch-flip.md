### JSON lazy tape: sequential scans hand off to the batch parser (#7478)

A full scan of a `JSON.parse`'d top-level array was invisible to the lazy
tape's only adaptive signal. `lazy_get`'s `cumulative_walk_steps` counter
trips at `2n`, but a sequential walk costs exactly one tape step per element
and so accumulates only `n` — it can never fire on a scan, by construction.
Every element was therefore materialized individually, at roughly 1.8× the
batch parser's per-element rate, and #7499's batch reparse (gated on the
sparse cache still being mostly empty) was only ever consulted *after* the
scan had filled the bitmap, where it is a deliberate no-op.

`LazyArrayHeader` gains `sequential_streak`, a run-length of consecutive
ascending cold reads, tripped by `scan_flip_threshold` — `n/64`, floored at
64. The floor is what keeps a glance at the first few records from dragging
in a parse of the whole document; the proportional part keeps the evidence
scaled to the array. The trigger also carries `force_materialize_lazy`'s own
`cached_count * 2 < cached_length` test, so it never asks for a batch
producer the callee would decline (which would otherwise fire on a 64–128
element array, where the streak can only complete near the end).

Measured on the pinned quiet host (M1 mini, 15 runs, `taskpolicy -t 0 -l 0`,
both arms compiled with `PERRY_NO_AUTO_OPTIMIZE=1` and run back to back):

| workload | before | after |
|---|--:|--:|
| `json_polyglot` roundtrip | 201 ms · σ1.3 · 87 MB | 201 ms · σ1.6 · 87 MB |
| `json_polyglot` field_access | 2938 ms · σ135 · 222 MB | 2043 ms · σ146 · 195 MB |
| parse + full scan, no stringify | 2729 ms · σ215 | 1339 ms · σ26 |

Checksums identical to node on both arms. The roundtrip arm's
unmutated-blob memcpy path is untouched by construction — a workload that
never indexes never opens a streak. On a pure scan the tape now beats its
own tape-off arm (1339 vs 1545 ms), so it has stopped being a net negative
on that shape.

**field_access does not reach the `idiomatic` floor** (2043 ms against
1478 ms with the tape off and mark-sweep), and that is structural rather
than a tuning miss: the tape build is purely additive whenever the whole
tree ends up materialized anyway, and the flip hands off to a producer that
re-tokenizes the blob from scratch. Turning the tape off for this shape is
still ~500 ms better than this change.

Decomposing the four `PERRY_JSON_TAPE` × `PERRY_GEN_GC` combinations (the
public baseline's `idiomatic` row flips both at once, so "tape off" alone
had never been measured) also relocates the remaining cost. The run-to-run
variance and the RSS are a **generational-GC × tape interaction**, not the
materializer: with the identical tape, switching to mark-sweep moves scan
σ from 214.9 to 8.8 and RSS from 208 MB to 66 MB, while tape-off under
gen-GC is σ17.6. Each parse allocates header+tape as a single ~2.4 MB block
(200,002 tape entries for the 10k-record fixture) that rounds up to a
dedicated oversized arena block and is then retained. After this change
`tape + mark-sweep` field_access is 1759 ms at σ2.1 and 76 MB, so the
element-wise materializer is no longer the bottleneck — releasing the tape
and blob once `materialized` is set is.

Tests moved to `crates/perry-runtime/src/json_tape_tests.rs` (`#[path]`
sibling) to keep `json_tape.rs` under the 2,000-line cap. The six new cases
assert the producer that ran, not just the values: `reparse_materializations()`
tells the batch reparse apart from the element-wise merge walk, which
produces identical output, so a test that only checked the tree would pass
against the old never-flips behaviour. `LazyArrayHeader::cached_length`'s
offset-0 codegen contract — Perry inlines `.length` as a raw u32 load there
rather than calling `js_array_length` — is now enforced by a `const`
assertion rather than a doc comment, since this change adds a field to that
struct.

Surfaced two pre-existing defects, both filed rather than fixed here: the
element-wise materializer can lose an object's key pointers under tape +
generational GC, so `JSON.stringify` emits `field0`/`field1` instead of the
real names (#7538 — this change reduces the exposure by retiring that path
for scan shapes, but does not fix it), and
`PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_ZEAL=1` faults in
`test_json_tape_eager_materialization_handles_survive_copied_minor_gc`,
A/B-confirmed identical on the merge base.
