**`perf(gc)`: the copying minor stopped traversing the young graph twice — the eligibility preflight is skipped when its answer is already known (#7645).**

`CopiedMinorEligibility::evaluate` walked the whole live young object graph before every copying minor, and the collector then walked it again to copy. The first walk produced no collection result. It answered two booleans:

1. is any transitively reachable `Eden`/`FromSurvivor` object `GC_FLAG_PINNED`? (`CopyingNurseryPreflight::check_ptr_with_reason`)
2. was a non-arena candidate met while the malloc registry was unavailable **and** non-empty at cycle start? (`CopyingPointerSet::classify_for_preflight`)

(2) is already O(1) — `malloc_registry_available || malloc_registry_empty_at_start`. (1) is O(live young graph) only because it *searches* for a fact that can be *recorded when it is created*. `gc::pin_object` is now the single sanctioned setter of `GC_FLAG_PINNED` and arms a process-wide **monotone** latch when — and only when — the pinned object sits in a space the copying minor relocates. With the latch clear and (2) decided, both walks provably return `None` and are skipped. Note the direction: "no young pinned object exists at all" is *stronger* than the walk's "none is reachable", so the substitution is conservative rather than merely equal. When either proof is unavailable the walks run exactly as before, so the decision is never changed — only skipped when its outcome is already determined.

Measured on the pinned quiet mini, both arms built there, interleaved, 6 rounds, `PERRY_NO_AUTO_OPTIMIZE=1` with a pinned `PERRY_RUNTIME_DIR`, output SHA-256 identical on every row:

| `json_pipeline` | 200k | 500k |
|---|--:|--:|
| `build_out` phase | 622 → **489 ms (−21.4%)** | 1,659 → **1,245 ms (−25.0%)** |
| total wall | 1,004 → **866 ms (−13.7%)** | 2,606 → **2,190 ms (−15.9%)** |
| `parse` / `serialize` | flat (−0.4% / −1.4%) | flat (−0.2% / +0.9%) |

Spreads do not overlap in any moved cell (500k `build_out`: base 1,651–1,672, arm 1,238–1,250).

**The subject was live and the decision unchanged.** `PERRY_GC_DIAG` on the arm reports `eligible=true fallback=none preflight_skipped=true (skips=1 walks=0)`, and `promoted_objects=4,117,015` / `promoted_bytes=280,996,840` / `freed_bytes=17,544` are byte-identical to the base arm. A field-by-field `PERRY_GC_TRACE` diff of all three cycles — 1,868 non-timing fields — shows **8 differences, all telemetry of the removed traversal**: `layout_scans.pointer_slots_read` 22,041,102 → 13,827,564, `unknown_layout_slots_read` 16,500,006 → 10,500,003, `masked_pointer_slots_read` 3,014,775 → 1,810,014, `pointer_slot_bytes_read` 176,328,816 → 110,620,512, the three `pointer_free_*_skipped` counters halved, and `old_pages.dirty_slots` 1,017,546 → 508,773. Every counter describing the *collection* — cycle count, kinds, triggers, `copied_*`, `promoted_*`, `freed_bytes`, `remembered_set`, `root_sources`, `sweep` — is identical.

### The issue's pin-site analysis was incomplete, and that is the load-bearing finding

#7645 named three production `GC_FLAG_PINNED` setters and argued all three were harmless (malloc-space or `Longlived`). There are **six**, and three of them pin **Eden** objects:

- `perry-stdlib`'s `async_bridge::pin_promise_for_native_resolution` pins a `js_promise_new()` promise — an `arena_alloc_gc`, i.e. Eden, whenever promise hooks are off. Every `fetch`/`zlib`/`ws`/`bcrypt`/`ioredis` request goes through it.
- `perry-ui-macos`'s `textfield::get_string_value` and `table::get_filter_text` pin the `js_string_from_bytes` result they hand back to JS — also Eden.

The two AppKit sites wrote `*gc_flags_ptr |= 0x04;` against a hand-computed `ptr - 8 + 1`, so **they are invisible to `grep GC_FLAG_PINNED`** — which is how an enumeration done by grep came back short by half. That is the argument for the gate being a scanner rather than a list. It does not sink the approach (the latch handles those sites; they arm it), but it changes the honest claim about who benefits: `perry-stdlib`-async and AppKit programs keep today's behaviour, compute- and JSON-shaped programs get the walk removed.

### Three enforcement layers, because a wrong latch is a use-after-move

`move_young` relocates a pinned object exactly as it would any other — it only *preserves* the bit — and the cross-thread promise queue holds a raw `usize` no scanner rewrites.

1. **Static, in `lint`.** `scripts/gc_pin_sites.py` fails on any site that originates a pin outside `pin_object`, matching the named form *and* any write into a `gc_flags`-named identifier whose right-hand side carries an integer literal with bit 2 set. It fails equally on a **stale allowlist entry** (the `deferred_registration_flush_sites` model in `arena/tests.rs`), and refuses to report green having seen fewer than 40 `GC_FLAG_PINNED` tokens. `--self-test` plants six offender shapes, requires each to be caught, and requires the read/clear/preserve shapes not to be. The two flag-byte channels it deliberately does not scan — allocator birth flags (`GC_BIRTH_EXTRA_FLAGS` is only ever `0` or `GC_FLAG_MARKED`) and codegen's inline bump allocators (`GC_FLAG_ARENA` plus that same byte) — are documented in the script with why neither can originate a pin.
2. **Dynamic, at the instant it would matter.** `move_young` already holds the flags byte in a register; on a *preflight-skipped* cycle it tests bit 2 and aborts with `[gc-pin-latch] FATAL`, naming the header, rather than relocating it. One `and` and a never-taken branch. Deliberately *not* applied when the preflight ran: that path is unchanged here, and a divergence between the preflight's traversal and the copier's would be a separate bug that should not newly abort a program.
3. **Tests.** Every pinned-fallback test plants its pin through `pin_object`, so none can pass on an unsound configuration. `gc/tests/copying/latch.rs` adds the skip/liveness case, the `Longlived`- and malloc-pin cases that prove the `SMALL_INT_CACHE` and `spawn`'s cross-thread promise stay free, the monotonicity case, and a subprocess sabotage test that plants a raw young pin and requires the collector to die on `SIGABRT` with that message.

**Sabotage-verified.** Deleting the one `YOUNG_PIN_EVER.store(true, …)` line and running each protection test alone: `young_pin_via_pin_object_restores_the_walk` and all three `test_copying_minor_falls_back_for_pinned_young_*` cases die with `SIGABRT` from layer 2; `the_latch_is_monotone_across_an_unpin` fails its assertion. The control case (`no_pin_ever_means_the_preflight_walks_are_skipped`) still passes, so the sabotage broke the protection and not the harness.

### Why monotone

A decrementing counter would recover the fast path after a transient pin (a settled `fetch` promise). It was rejected because it adds a *second* completeness obligation of the same severity — every unpin site, where a spurious or double decrement is silently unsound in exactly the same use-after-move way. Monotone needs one proof. The cost is stated and asserted by `the_latch_is_monotone_across_an_unpin`: a process that ever pins young pays the walk for the rest of its life.

### One ordering hazard found and closed

`dirty_slot_preflight_reason` took a `remembered_dirty_snapshot()`, whose **first** call on a thread arms the barrier and rebuilds the remembered set from the heap — a walk whose own comment says "nothing is marked yet when a collector first asks for the log". In a successful copying minor that first call was always the preflight's. Letting it fall through to the copy phase's snapshot would have run it after `visit_mutable_root_slots` had already evacuated root-reachable young objects, i.e. against a half-moved heap. `arm_and_reconstruct_remembered_set_if_unarmed()` is therefore called explicitly on the skip path, keeping it where it was. It is one-shot per thread, so every later cycle pays a thread-local flag read.

### gc-ratchet: one cell re-pinned

`01_nursery_churn.heap_used_bytes` moves 6,277,048 -> 7,325,584 and is re-pinned in `benchmarks/gc_ratchet/baseline/gc-ratchet-v1.json` (one cell; `tolerances.json` untouched). It is **not** retention: `gc_ratchet.py classify` reports `heap_used_precise_bytes` byte-identical on all 12 probes and on both arms (5,228,512 here), and the whole delta is `false_root_excess` — exactly one 1 MiB nursery block. Cause is #7558: the probe's own `gc()` forces a conservative stack scan, and removing the preflight's recursion changes which stale pointer-shaped words survive deep on the native stack, where one of them pins a whole block.

It is re-pinned rather than given `12_large_live_set`'s `probe_overrides` exemption because the delta is **deterministic** — spread 0 over 7 samples on both arms — and a reproducible shift can still carry a band, whereas that exemption's premise is genuine sample-dependence and `gating` is one-way. Same host as the rest of the artifact (checked against its `host` block), and `check` was green for current main against the unedited artifact, so the other 143 cells are still in band. The re-pinned cell still fails on a further 1 MiB block, so it remains a live gate.

### Counters that move, deliberately

Skipping a traversal removes its telemetry, and only that: the eight fields above. `test_copying_minor_rewrites_exact_{object,closure}_pointer_*` now expect `masked_pointer_slots_read == 1` instead of `2` — one read by the copier where there used to be one by each walk — so the drop has a unit-scale witness that fails if the walk ever returns.

New: `trace.copying_nursery.preflight_skipped`, `gc::copied_minor_preflight_skips()` / `copied_minor_preflight_walks()`, and a `PERRY_GC_DIAG` line, so a verdict about this change can assert its subject was live (#7024/#7025) instead of passing on a cycle that never skipped anything.
