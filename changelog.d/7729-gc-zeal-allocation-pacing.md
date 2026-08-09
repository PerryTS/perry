### Fixed

- **`PERRY_GC_ZEAL=1` terminates again — it is now allocation-paced instead of collecting at every back-edge poll (#7728).**

  Zeal is the primary instrument for moving-GC correctness bugs, and half of the pairing that produced the precise fault behind #7682. It had stopped completing on real workloads: `gc-handoff/apps/iso_miss.ts`, a tree-walking interpreter, timed out at 240 s with no output under zeal (it runs in 4.5 s on the quiet bench host without it).

  **It was never a livelock, and the last-known-good build was never good.** Scaling the round count on the pinned quiet host gave 70,968 forced collections in 36.3 s for one round and 141,931 in 72.7 s for two — perfectly linear, so the full workload was ~24 minutes rather than a hang. Zeal forced a collection at *every* back-edge poll: ~511 µs of fixed per-collection cost (root scan over the shadow stack plus ~55 side-table scanners) to relocate a mean of **5.9 objects**. Nearly all of the work was the collection's fixed overhead, not the relocation zeal exists to stress.

  The earlier build that ran "instantly with the correct answer" was **vacuous**. `PERRY_GC_MOVING_LOOP_POLLS` was default-OFF there (#7161), so a compute-only program reached no loop safepoint and zeal forced nothing; that build also predates the #7604 exit verdict, so it exited 0 in silence. #7721 flipped the poll default ON — correctly, it is a large collector win — and in the same commit turned zeal from free-and-vacuous into correct-but-unusable. Isolated rather than assumed: the *old* compiler with `PERRY_GC_MOVING_LOOP_POLLS=1` forced at compile and run time already costs 35.8 s for one round under zeal against 0.62 s without, so no commit broke zeal — zeal was never paced, and the poll default is what exposed it. #7254 had already logged "a striking concentration of multi-minute-plus runs" under this pairing and left the population untriaged; this is that triage.

  Zeal now forces a collection at the first poll at which `PERRY_GC_ZEAL_ALLOC_KB` (default 4) of new nursery material has accumulated — the model V8 (`--gc-interval`) and SpiderMonkey (`gcZeal(mode, frequency)`) both use, and for the same reason. The stride is a **monotone high-water mark**, not a "bytes since" delta: each forced collection rearms to `from_space_after + stride`, so a collection that reclaims nothing (an escalation to a non-moving full mark-sweep, which #7592 and #7682 both produced in the field) still demands another full stride of genuinely new allocation. Total forced collections are bounded by `bytes_allocated / stride` whatever the collector does with them, which makes this a bound rather than a hope.

  **Measured on the pinned quiet host.** The full workload under zeal goes from a 240 s timeout (unpaced cost ~1,426 s) to **98.8 s with the correct answer**, forcing 193,087 collections out of 2,838,560 polls — all of them copying minors, relocating 3,115,719 objects. The stride sweep below is one binary and one env var, at a quarter scale, and `loop_polls` is **283,852 in every row**, so the knob changes only the decision to collect:

  | `ALLOC_KB` | forced collections | moved objects | wall |
  |---|--:|--:|--:|
  | 0 (pre-fix) | 283,857 | 1,629,647 | 142.6 s |
  | 1 | 70,929 | 815,460 | 36.1 s |
  | **4 (default)** | **19,314** | **325,830** | **10.2 s** |
  | 16 | 5,070 | 129,959 | 3.0 s |
  | 64 | 1,291 | 52,357 | 1.1 s |

  Row 0 reproduces the pre-fix 1:1 behaviour exactly on the shipped binary. Every row keeps `copying_minors == forced_collections` and `moved > 0`, so no stride degrades the instrument into non-moving sweeps. 4 KB rather than the faster 16/64 is deliberate — this is a correctness instrument, so the default errs toward sensitivity, still collecting once per ~15 loop iterations while being 14x cheaper than unpaced. The zeal-OFF path is untouched: the same workload without zeal is 4.49 s before and after.

  `PERRY_GC_ZEAL_ALLOC_KB=0` restores the literal every-poll semantics — the right setting for a small fixture or a bug window executed exactly once. What pacing gives up, stated rather than buried: a window crossed a single time may now fall between two forced collections; a window that *recurs* (every shape in the #7154 family, which is why the reproducers are loops) is still caught, after N KB of allocation instead of on the first iteration.

### Changed

- **`scripts/gc_instrument_smoke.sh` gains a budgeted zeal-termination arm, and pins every-poll mode for its existing arms.**

  The gate ran zeal end-to-end and was green throughout. It could not see the regression: its fixture is deliberately sized at ~1200 polls "so the zeal arm costs seconds rather than minutes", and at that size every-poll and paced are indistinguishable. Arm 6 runs a 200k-iteration workload at the *shipped default* (with `env -u`, so the file's every-poll pin does not apply to it).

  **The discriminator is the collections-to-polls ratio, not the clock**, and that is a deliberate choice. Measured, the fixture is 0.49 s paced against 11.85 s unpaced — a real 24x, but both fit inside any budget loose enough not to flake on a shared runner, so a wall-clock assertion alone would be decoration. The ratio is host-independent and exact: the regression's signature is one forced collection per poll (measured 200,069 for 200,064), the shipped default is 1-in-40, and the gate fails above 1-in-4. The budget survives as the weaker "does it terminate at all" guard.

  Two things had to be fixed for the arm to mean anything, both found by running it rather than reasoning about it. Its records must **escape**: the obvious fixture allocates a record per iteration and drops it, which scalar-replaces into nothing — 6 forced collections and 17 moved objects over 400,000 polls, an arm that ran the loop but never gave the collector anything to relocate. Pushing into a bounded rolling array takes `moved_objects` from 17 to 640,364. And the arm is sabotage-checked: forcing it back to every-poll makes it fail with the 1:1 signature.

  It asserts non-vacuity too — forced collections, copying minors and moved objects all non-zero — because "fast because it collects nothing" would be a worse regression than the slow instrument it replaced. Arms 1–3 and 5 now set `PERRY_GC_ZEAL_ALLOC_KB=0` explicitly, so they keep testing the strongest semantics rather than silently following whatever the default becomes.

- **CLAUDE.md's instrument table** documents the pacing knob and loses a stale claim: it still said loop back-edge polls were "default off since #7161", which #7721 had made false. That sentence is why zeal's real cost was invisible.

### Tests

- `zeal_pacing_bounds_forced_collections_but_still_moves_objects` — the regression test, in the required `cargo-test` gate. Drives a hot poll loop and asserts forced collections stay bounded well below the poll count, paired with two liveness assertions (collections still forced, survivors still moved). Sabotage-checked: pinning the stride to 0 fails it with `2000 forced for 2000 polls`, exactly the pre-fix ratio.
- `zeal_alloc_stride_zero_restores_every_poll_collection` — the OFF state of the new knob, per the binding GC knob kill-policy.
- `zeal_pacing_rearms_above_survivors_so_a_useless_collection_cannot_loop` — pins the monotone high-water mark against a delta-based rewrite that would restore the livelock shape.
- `zeal_alloc_stride_knob_parses_both_states` — including that `0` is a meaningful value rather than garbage to be defaulted away.
