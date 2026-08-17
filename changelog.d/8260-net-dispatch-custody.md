### ext-net: the pump's in-flight dispatch gets GC custody (#8259)

Every arm of `js_ext_net_drain_pending` snapshotted its listeners into a bare
`Vec<i64>` and, for some arms, allocated a payload before the call loop. Both
steps can collect — and the evacuating arms then MOVE the closures:
`scan_net_roots` rewrites the canonical `statics::listeners()` slots, but a
bare local snapshot is invisible to it, so callback N (and the already-built
payload) were dereferenced at their OLD addresses once callback N-1's JS
forced a collection. Symptom: `test_gap_gc_net_once_flags_rekey` (the #8216
witness, which had never completed a CI run) dies under `force_verify` —
SIGSEGV on Linux, `TypeError: value is not a function` on macOS.
`ServerListening`/`ServerClose` were worse: they remove their callbacks from
the table before firing, so nothing rooted them at all during dispatch.

Fix is the #8216 H2 custody pattern: park the snapshot (+ at most one
NaN-boxed payload per frame) in scanned thread-locals
(`crates/perry-ext-net/src/dispatch_custody.rs`), re-read each slot
immediately before use so the copying GC's rewrite is observed, pop on drop.
All ten dispatch loops converted; the census call-graph walk certifies the
new holders through `scan_net_roots`.

Validated by A/B under the matrix's exact `force_verify` env (8 MB pressure):
pre-fix rc=1 with the stale-closure TypeError, post-fix rc=0 and byte-correct
`once fired: 1 of 2 events`, copying minors live in both arms.
