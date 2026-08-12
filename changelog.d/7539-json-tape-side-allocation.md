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

  The cleanest attribution is `bench.ts` (roundtrip), which never materialises anything: its nursery peaks at **4.1 MB** while the old generation peaks at **39.6 MB** and fires 5 `old_gen_bytes` fulls, identically under both collectors. In that program there is nothing in the old generation *but* the tape. That measurement is what promoted the issue's hypothesis to a cause — and it also ruled out the RSS-pressure theory the headline numbers first suggested: `evacuation_policy` reports `not_evaluated` on every cycle of every arm, and evacuation moved 0 bytes.

  **The fix.** The tape moves out of the GC heap into a `json_tape_store` side allocation, which the header owns. It qualifies on every test already applied to `Map`/`Set` entry buffers: it is **pointer-free by construction** (`TapeEntry` is `{ offset: u32, kind: u8, link: u32 }` — the struct's alignment is 4, so on a 64-bit target no field it has can hold a pointer, and the region has exactly one writer), **uniquely owned** by one header, and immutable and immovable after construction. So it never needs marking, scanning, copying, or rewriting.

  On top of the collector-driven lifetime, the owner disowns its tape **deterministically**: the instant `force_materialize_lazy` installs `materialized`, every subsequent read goes through the `ArrayHeader` and the tape is provably garbage, so it is freed right there with no collector involved. That is the path `field_access` takes — #7537 flips the scan to the batch parser after `scan_flip_threshold` elements, a few hundred of 10 000 — which is why the result does not depend on GC timing for the workload that motivated it. Every site that sets `materialized` now goes through one `install_materialized` helper so the release cannot drift away from the install.

  **The header stays exactly where it was, and that is load-bearing.** The first version of this change let the shrunken ~88-byte header fall into the nursery, which made it MOVABLE for the first time in its life — its multi-megabyte inline tape had always parked it in the old generation. Callers outside `json_tape` were written against that: `json::stringify_api::try_stringify_lazy_array` reads `blob_bytes` off a raw header and then allocates the result string, and the array accessors pass raw headers into `force_materialize_lazy`. The copying minor promptly relocated the header out from under them, and `field_access` went non-deterministic — `JSON.stringify(parsed)` returned a JSON string of NUL bytes (a stale `blob_str` read through a moved-from header) on 3 of 60 iterations, while every element value stayed correct. So the header is now allocated old-gen and born tenured *explicitly* (`arena_alloc_gc_old_born_tenured`), stating the invariant instead of relying on the tape to imply it, and `GC_TYPE_LAZY_ARRAY` is marked non-movable so old-page defrag can never change it. That also means the tape registry needs no move hook and no copied-minor from-space pass. The cost is ~96 bytes of old generation per parse instead of ~2.4 MB.

  The sparse element cache and its bitmap move to the same generation for the same reason. An old-gen header with a nursery cache is a mix nothing covers: a minor treats the old header as a black leaf, so it never visits the `GcRewriteDescriptorKind::LazyArray` descriptor — the only thing that can read the cache — while the cache block is itself a GC leaf whose contents no walker scans. That combination lost element identity (`parsed[i] === parsed[i]`) across a copying minor. It could not arise before: a big array's cache was already born old, and a small array's header was born young alongside its cache.

  **One accounting trap, and why it is not one.** Tape bytes stay in `external_side_live_bytes()`, which every `old_reclaim_pressure_due` call site adds to old-generation pressure. That looks like it would re-create the pathology, and an intermediate version of this change removed it for exactly that reason. It was the wrong call: the old cost was *dead* tape sitting in the old generation as unreclaimable arena capacity, and `field_access` now disowns each tape at materialization, so the term never accumulates there — it holds one live tape at a time. `roundtrip` genuinely retains its tape until the lazy array dies, and those bytes must be able to escalate the reclaim that frees them, exactly like a dead Map's entries buffer; it keeps the bounded cadence it has today.

  **Result.** Pinned quiet host (M1 mini, `taskpolicy -t 0 -l 0`, 11 runs, load ≤ 2), same binaries end to end:

  | `field_access` | median | σ | peak RSS |
  |---|--:|--:|--:|
  | before, default | 1957 ms | 143.8 | 196 MB |
  | **after, default** | **1809 ms** | **17.3** | **155 MB** |
  | before, `PERRY_GEN_GC=0` | 1751 ms | 4.2 | 76 MB |
  | after, `PERRY_GEN_GC=0` | 1604 ms | 3.0 | 76 MB |
  | after, `PERRY_JSON_TAPE=0` | 1758 ms | 117.2 | 168 MB |

  σ collapses 8.3×, which was the headline symptom, and RSS drops 41 MB. The decisive row is the last one: **turning the tape ON is no longer worse than turning it OFF** — the tape-off arm still carries σ 117.2 and 168 MB, so the residual variance and footprint are the generational collector's own behaviour on this workload and have nothing left to do with the tape. The GC trace agrees exactly: `field_access` goes from 19 cycles / 9 full / 6 `old_gen_bytes` to **14 / 5 / 2**, which is the `PERRY_JSON_TAPE=0` arm's profile to the cycle.

  `roundtrip` — the memcpy path this must not regress — improves: **201 → 193 ms** (σ 0.5 → 0.7), and its peak old-generation *in-use* falls 39.6 → 14.1 MB with reserved 58 → 26 MB. It keeps its 5 `old_gen_bytes` fulls, by design: it genuinely retains each tape until the lazy array dies, so those bytes should keep escalating the reclaim that frees them.

  The change also helps the mark-sweep arm (1751 → 1604 ms), which is worth noting because it is independent of GC pacing: an inline multi-megabyte allocation per parse cost real time regardless of collector.

  Not claimed: the ~200 ms and ~79 MB still between the default and `PERRY_GEN_GC=0` on this workload. The tape-off arm carries the same gap, so it is a separate term.

  **Coverage.** `gc/tests/lazy_tape_side_alloc.rs` pins the load-bearing claims: old-generation growth no longer scales with tape size (two blobs of the same element count whose tapes differ 3×, so the blob string and the sparse cache are held constant — measuring one parse against zero would only have proved that old-gen grew by *less* than the tape); the header is small but still old-gen, born tenured, and non-movable; a dead unmaterialized owner releases its tape on a full collection; and a minor neither releases nor moves a live owner's tape. `json_tape_tests.rs` pins the pointer-free claim structurally rather than by convention, and that a disowned tape reads as empty rather than as freed memory. `gc/tests/teardown.rs` covers thread exit.

  `PERRY_GC_VERIFY_EVACUATION=1` runs clean on both benchmarks and on a 60-iteration divergence probe, including with `PERRY_GC_FORCE_EVACUATE=1`. All 42 JSON/lazy `test-files/*.ts` that build under `PERRY_NO_AUTO_OPTIMIZE` match `node --experimental-strip-types` (26.5.1) byte for byte, and both benchmark checksums are identical to `main` across repeated runs.

  Two existing tests asserted that a copied minor *relocates* the lazy header — true only because their tiny fixtures made the header small enough to be nursery-resident, which production never was. They now assert the opposite, which is the real invariant, and keep their live half: the materialized array is young, does move, and its handle must still be refreshed.
