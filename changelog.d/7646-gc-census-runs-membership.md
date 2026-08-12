**`perf(gc)`: the full collection's valid-pointer census kept a `BTreeSet` shadowing the run vector it was already building (#7592).**

`ValidPointerSet` held two structures over the same addresses. `arena_runs` — the census walk's arena object starts, handed over by `ArenaObjectCursorBuilder::new(ArenaWalkOrder::Address)` in **ascending address order** — was documented as existing only for `enclosing_object`'s floor lookups. `lookup_set: BTreeSet<usize>` answered exact membership, fed by the *same* `push_arena` call. But the runs are sorted by construction, so a floor lookup that lands **on** the query already is the membership answer: the B-tree was buying nothing except one insert per live arena object.

On `json_pipeline` 500k that shadow cost **245.5 ms of a 748.3 ms full collection** (the GC's own `phase_us.build_valid_pointer_set`) — 12.6% of the `build_out` phase, and the second-largest single leaf in the symbolicated profile.

Membership for arena starts now comes from the runs; the B-tree keeps only malloc-tracked starts, which have no address order to exploit and are skipped entirely when empty. `lookup_count()` preserves `snapshot_for_tests`'s meaning (arena starts + malloc starts).

**One part is load-bearing and is recorded so it is not "simplified" away.** A first cut searched `arena_runs: Vec<Vec<usize>>` directly, calling `run.first()` per probe. That arm moved `phase_us.trace_worklist` the *wrong* way — 388.8 → 493.1 ms, +99 ms over ~9.6M lookups — and netted only −6.3% on `build_out`, which reads like "the B-tree was buying something". It was not; it was buying an indirection. Mirroring each run's first key into one contiguous `arena_run_firsts` vector (~4k entries, 32 KB, L2-resident, against 33 MB of run storage) turned that +99 ms into −62 ms and took the change from −6.3% to −15.4%.

Measured on the pinned quiet mini, interleaved A/B, 7 rounds, `PERRY_NO_AUTO_OPTIMIZE=1` with a pinned `PERRY_RUNTIME_DIR`:

| | 200k | 500k |
|---|--:|--:|
| `build_out` phase | 731 → **620 ms (−15.2%)** | 1,956 → **1,654 ms (−15.4%)** |
| total wall | 1,155 → **1,041 ms (−9.9%)** | 3,008 → **2,703 ms (−10.1%)** |
| the full collection's pause | 294.4 → **182.8 ms (−37.9%)** | 750.3 → **458.3 ms (−38.9%)** |
| ↳ `build_valid_pointer_set` | 94.3 → **9.1 ms** | 247.0 → **23.4 ms** |
| ↳ `trace_worklist` | 152.9 → **129.2 ms** | 388.8 → **326.4 ms** |
| peak RSS | 461.7 → **446.2 MB (−3.3%)** | flat (±1%) |

Output SHA-256 identical at both sizes. No policy, pacing, trigger, promotion or root-set behaviour is touched — the membership *set* is unchanged, only its storage and probe. `PERRY_GC_TRACE=1` on both arms at both sizes, comparing **every non-timing key of every `gc_cycle` event field by field: 0 differences** (same cycle count, kinds, triggers, `promoted_*`, `freed_bytes`, `copied_*`, `remembered_set`, `old_pages`, `layout_scans`, `root_sources`, `sweep`).

Found while re-deriving the next `json_pipeline` lever after #7624 and #7633. The **larger** item on the same profile — the copying minor's eligibility preflight, a second full traversal of the young graph worth 21.8% of `build_out` — is filed as #7645 rather than taken here: it is a guard on the moving collector and needs a pin-site completeness gate, a sabotage test and a deliberate ratchet counter shift first.
