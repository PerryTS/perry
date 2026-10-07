Store WeakMap/WeakSet entries and identity buckets in a GC cell owned by the
collection, with the intrinsic brand in its Shape. Carry one validated lookup
view and one key classification through get/has/set/delete. Remove the external
owner-address index and repeated generic entries-array validation.

Fix conditional ephemeron tracing (#12087): follow values only after their
keys are independently live. Value-to-key cycles clear; moving collectors
rewrite both words and rebuild buckets. Preserve incremental weak-write
coverage, WeakRef/FinalizationRegistry behavior, symbol rules, exotic keys,
receiver checks, subclassing, and reflection.

Fresh qb6 validation on main 68e41faa (including #12105): 5208 release runtime
tests and the integration test pass. Both #12087 regression witnesses fail
at the retention assertions on main and pass with fix. All six previously
Node-matching weak-area files remain matching; the new behavior matrix passes
under 57 copying minors moving 38,421 objects.

Performance acceptance remains blocked by TypeScript CPU: instructions
+0.195%, cycles +4.082% beyond a 0.186% identical-binary floor; THP disabled
still gives +2.294% cycles beyond a 1.021% floor. The actual TypeScript source
now builds and matches Node. WeakMap get uses 70.8% fewer instructions;
exact Effect uses 6.9% fewer and 7716 KiB less peak RSS. Its extra minors
reflect conditional retention and unchanged nursery retuning, while full
collections fall 2 to 1. The old control-RSS gate has been superseded.

See docs/src/internals/weakmap-owned-index-validation.md for all 17 rows,
CPU/RSS/minor/full counts, same-binary floors, miss diagnostics, Node/Bun
references, known main-only check failures, and the remaining layout issue.
This is an unaccepted review candidate; do not publish it as a passing
performance change before that CPU issue is resolved or reviewed by the owner.
