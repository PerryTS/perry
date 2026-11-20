Killed the per-element linear key scan behind dynamic string-keyed property
access: **−61% on baseline dynamic property throughput** (768 → 303 ms on the
`bench_dynamic_property_keys` overwrite loop; node on the same host: 12 ms).

The `[[Set]]`/`[[Get]]` fallback walks and the `delete` path each carried their
own copy of the same loop: `for i in 0..key_count { js_array_get(keys, i) +
string compare }` — the full JS-facing array accessor (pointer cleaning,
typed-array and buffer registry probes, descriptor gates) per element, per
property operation. The pointer-keyed read plan in front never hits for a
computed key, because `o["k" + i]` allocates a fresh key string every
evaluation. Counted with a temporary in-runtime counter: **90.8 million
`js_array_get_f64` calls for 1.5 million property operations** (~60 per
access). After this change: 15.1 M, all of it in `delete`'s array compaction
rather than lookup.

The scans now go through one shared helper (`keys_find_slot_by_bytes` /
`_by_key_ptr`): the shape hash index (`shape_slot_lookup`, content-validated)
answers in O(1) when present, with a raw dense-slot linear scan (no per-element
accessor) as the fallback and correctness backstop.

Two hazards were found by testing and are baked into the design:

* **Consult-only (`build=false`).** A delete drops the shape index; rebuilding
  it on the next access to use it once **doubled** delete-heavy time
  (1570 → 3064 ms measured) while the call counter barely moved — the time went
  to rebuilds, not scans. These sites therefore only consult an index the write
  path already maintains incrementally; churny receivers fall back to the raw
  scan instead of thrashing rebuilds. Final: delete-heavy 1497 ms vs 1433
  baseline (within the ±10% noise band of that metric), overwrite keeps the
  full win.
* **Garbage-length tolerance.** The old loops compared LENGTHS first
  (`js_string_key_matches`), so a `key` pointer that is not a valid string
  header was a harmless mismatch. The first helper version built a slice from
  that length and panicked in an unrelated stream test with `range start index
  2613749136200 out of range`. The helper now sanity-checks
  `byte_len <= capacity && < 2^28` before slicing and otherwise falls back to
  the length-guarded compare.

Also switches the shape-scanner probe memo (#8899) off std's SipHash: perf put
`RandomState::hash_one::<&(usize, bool)>` at **7.0% of total samples** on this
workload. The key folds to one word (`addr | carrier_bit`; addresses are
8-aligned so bit 0 is free) under `PtrHasher`.

Suites: macOS **2762 passed, 0 failed** (complete); Linux x86_64 2654 passed,
0 failed with the `node_stream` error-path family excluded — that family
aborts identically on clean main (verified by stash), a pre-existing
Linux-specific failure reported separately.
