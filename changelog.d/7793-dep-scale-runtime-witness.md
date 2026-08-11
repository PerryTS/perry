**The moving collector now has a dependency-scale RUNTIME witness in CI** (#7717, the one unmet ask of #7280).

Everything gating this family was either a synthetic fixture or a static pass over emitted IR, and neither can observe what the #7154 class actually presents as. `scripts/gc_root_dominance_check.py` reads LLVM IR, so a runtime-side `static` or `thread_local!` holding a raw heap pointer is structurally invisible to it; and the defect is invisible to every runtime GC probe *at* the collection too, because there is nothing for the collector to find — it surfaces cycles later, in a different function, as `TypeError: value is not a function`.

Scale is the point, not size. #7280 measured it: the curated 25-file corpus passed 25/25 while twenty lines of stock `zod` failed 5/40, because dependency-shaped code is dominated by `js_object_assign_one` (object spread) and `js_new_function_construct` — populations the curated files barely produce.

`scripts/gc_dep_scale_witness.sh` runs `test-files/gc-dep-corpus/main.ts` — the same 81-module zod corpus `gc-root-dominance` already compiles for its static check, whose own header always said it "is run as the acceptance workload for the moving collector" — under `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, and is wired into `gc-moving-witnesses.yml`.

Measured on `main`: 8,020 forced collections, all 8,020 of them copying minors, 852,107 objects relocated, 63,936 back-edge polls, 8,020 retired quarantine page-sets, and the answer byte-identical to the same binary run without the schedule.

**What makes it able to fail.** The trap #7717 names is a run that exits clean having witnessed nothing, so the script asserts its subject ran three independent ways: the `[gc-schedule]` exit verdict; `copying_minors` / `moved_objects` / `loop_polls` re-read from it rather than trusting that the binary still exits 70; and a non-zero count of retired from-space page-sets, because `PERRY_GC_PROTECT_FROMSPACE=1` on a run with no copying minor protects nothing and still exits 0. Both directions were verified by sabotage rather than asserted: with the schedule disabled it fails on the missing verdict, and with the quarantine disabled it fails with "protected NOTHING and this run's cleanliness means nothing" — which is precisely the wrong conclusion #7717 records nearly recording.

Two findings worth keeping:

- **`PERRY_NO_AUTO_OPTIMIZE=1` is load-bearing here**, and is a deliberate departure from `gc_repsel_matrix.sh`, which omits it on purpose. Without it the compile reaches the auto-optimizer, which relinks the runtime as `features=async-runtime,web-fetch` — no `diagnostics`, and `diagnostics` is what emits the `[gc-fromspace-protect]` line this gate reads as its proof. The assertions are fail-closed so a stripped runtime goes red rather than quietly green, but red-for-the-wrong-reason is still a broken gate.
- **Depth 800 is load-bearing, not decorative.** The quarantine runs saturated on this workload (`sets_held=800/800`), so lowering it silently narrows the window in which a stale dereference is still catchable. The default of 4 is far too small for real code — #7154's own reproducer needed 800.

The gate is deliberately **not** added to branch protection's required contexts: a new gate has never been green, so promoting it immediately would block every open PR. It should be observed on `main` first, then promoted — leaving that second step undone is how `gc-stress` ended up reporting failures without blocking anything.

Also corrected while writing this: #7717 says loop polls are "default off since #7161". They are default **ON** since #7721 — `moving_safepoint_polls_enabled_from_env` is `!matches!(value, Some("0") | Some("off") | Some("false"))` — so the witness does not need to opt into them, and the run above confirms 63,936 polls fired without the flag.

Ported from `PERRY_GC_ZEAL` to `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`
during the merge audit: #7741 retired the zeal knob between this PR's authoring
and its landing, and the witness's fail-closed verdict assertion would have gone
red on the missing `[gc-zeal]` line — red for the wrong reason, which is still a
broken gate. Re-measured and re-sabotaged post-port; numbers in the merge comment.
