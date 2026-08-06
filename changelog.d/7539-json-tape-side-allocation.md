### Performance

- **The JSON tape no longer lands in the old generation, so `JSON.parse` stops firing `old_gen_bytes` full collections (#7539).** Split out of #7478's decomposition; with #7537's scan flip landed, this was the whole remaining `field_access` gap.

  **Mechanism, confirmed before it was fixed.** A `LazyArrayHeader` was allocated as ONE arena object with its tape copied inline after the header, so the whole allocation was as large as the tape — ~2.4 MB for the 10 000-record fixture (200 002 `TapeEntry`s at 12 bytes). That is 150× `LARGE_OBJECT_THRESHOLD_BYTES` (16 KB), so `arena_alloc_gc`'s large-object arm routed it straight into the OLD generation and stamped `GC_FLAG_TENURED` on it. Old-generation bytes are reclaimable only by a FULL collection, so a tape that dies at the end of its loop iteration still accumulated at ~2.4 MB per parse until `old_reclaim_pressure_due` fired (48 MB absolute, or 32 MB of growth).

  Being *large* is not evidence of being *old*. The header was born tenured on the strength of its size alone, which handed the collector's cheapest question — "did this die in the nursery?" — to its most expensive answer.

  `PERRY_GC_TRACE=1` over the 53 parses of `benchmarks/json_polyglot/bench_field_access.ts` at the parent commit:

  | arm | cycles | full | `old_gen_bytes`-triggered | peak old-gen |
  |---|--:|--:|--:|--:|
  | tape + gen-GC (default) | 19 | 9 | **6** | 43.9 MB |
  | `PERRY_JSON_TAPE=0` + gen-GC | 14 | 5 | 2 | 47.7 MB |
  | tape + `PERRY_GEN_GC=0` | 31 | 31 | **0** | 14.1 MB |

  The cleanest attribution is `bench.ts` (roundtrip), which never materialises anything: its nursery peaks at **4.1 MB** while the old generation peaks at **39.6 MB** and fires 5 `old_gen_bytes` fulls, identically under both collectors. In that program there is nothing in the old generation *but* the tape. That measurement is what promoted the issue's hypothesis to a cause — and it also ruled out the RSS-pressure theory the numbers first suggested: `evacuation_policy` reports `not_evaluated` on every cycle of every arm, and evacuation moved 0 bytes.

  **The fix.** The tape moves out of the GC heap into a `json_tape_store` side allocation, which the header owns. It qualifies on every test already applied to `Map`/`Set` entry buffers: it is **pointer-free by construction** (`TapeEntry` is `{ offset: u32, kind: u8, link: u32 }` — the struct's alignment is 4, so on a 64-bit target no field it has can hold a pointer, and the region has exactly one writer), **uniquely owned** by one header, and immutable and immovable after construction. So it never needs marking, scanning, copying, or rewriting.

  Lifetime follows the proven Map/Set shape — `GcFinalizeHookKind::LazyArrayTape` for the non-copying sweeps, a from-space pass for the copying minor (whose bulk reset skips per-object finalizers), `GcMoveHookKind::LazyArrayTape` to rekey an evacuated owner, and a thread-teardown release. The header is ~88 bytes now, so it is born in the nursery and the copying minor really does move it; the old multi-megabyte header never did.

  On top of that the owner disowns its tape **deterministically**: the instant `force_materialize_lazy` installs `materialized`, every subsequent read goes through the `ArrayHeader` and the tape is provably garbage, so it is freed right there with no collector involved. That is the path `field_access` takes — #7537 flips the scan to the batch parser after `scan_flip_threshold` elements, a few hundred of 10 000 — which is why the result does not depend on GC timing for the workload that motivated it. Every site that sets `materialized` now goes through one `install_materialized` helper so the release cannot drift away from the install.

  **One trap worth recording.** The obvious way to account the new bytes, `gc_note_external_side_alloc`, would have made the change measure as a no-op: it feeds `external_side_live_bytes()`, which all four `old_reclaim_pressure_due` call sites *add to old-generation pressure*. That is right for a Map's entries buffer, whose owner is typically tenured so only a full reclaim can free it, and exactly wrong for a tape. Tape bytes get their own counter, cross-checked against the registry by the test accessor.

  **Coverage.** `gc/tests/lazy_tape_side_alloc.rs` pins the four load-bearing claims: old-generation growth no longer scales with tape size (two blobs of the same element count whose tapes differ 3×, so the blob string and the sparse cache are held constant — measuring one parse against zero would only have proved that old-gen grew by *less* than the tape); the header is a small, untenured nursery object for a huge tape; a dead unmaterialized owner releases its tape under both the copying minor and the full mark-sweep, each asserting it actually ran the collector kind it names; and an evacuated owner keeps its tape, asserting the header genuinely moved. `json_tape_tests.rs` pins the pointer-free claim structurally rather than by convention.

  #7538/#7546's barrier test asserts a lazy owner that a MINOR trace treats as a black leaf — the only shape where the in-object/external distinction bites. Before this change that shape was the *default*; it is now reachable only by tenuring, so the test places the header in old-gen explicitly (`ForceOldGenLazyHeaderGuard`) rather than quietly becoming a nursery-header test, and probes a cache slot far enough in that header and slot are on different pages by construction instead of by the header having been multi-megabyte.
