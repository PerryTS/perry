**`fix(gc)`: the moving-loop poll now defaults ON in the code, not only in the doc (#7690, #7682).**

#7690 wrote the whole default-ON argument into two doc comments — the runtime's `moving_loop_polls_enabled_from_env` and codegen's `moving_safepoint_polls_enabled` — and changed neither body. Both still matched `1|on|true`, i.e. default OFF, and no test pinned the default in either direction, even though the runtime predicate had been factored out expressly to make it "unit-testable without touching process env". The runtime doc asserted "Codegen's `moving_safepoint_polls_enabled` mirrors this exactly — they MUST agree"; they did agree, at the value the doc said they no longer held.

That is not a slower configuration, it is a different collector. Nursery pressure has exactly two precise collection points: the loop back-edge poll and the outermost microtask-pump boundary. With no poll emitted a compute-only program reaches neither, so every nursery collection happened at the register-imprecise allocation point — where #7687 had just made it correctly non-moving. The shipped collector therefore had **no nursery evacuation at all**, and the trigger fell back to whole-arena full collections.

Measured on the pinned quiet mini, best-of-3 interleaved, `PERRY_NO_AUTO_OPTIMIZE=1` with a pinned `PERRY_RUNTIME_DIR`, against `a853135aa` binaries rerun back-to-back on the same host:

| bench | main `12e48edd6` | this | `a853135aa` | node | scriptc 0.0.22 |
|---|--:|--:|--:|--:|--:|
| tree | 5.10 | **1.63** | 6.00 | 0.45 | 4.80 |
| tree_wide | 7.26 | **2.12** | 12.38 | 0.89 | 7.01 |
| retain | 2.33 | **1.32** | 1.33 | 0.15 | 0.11 |
| churn | 1.00 | **0.46** | 0.66 | 0.16 | 0.75 |
| churn_alloc | 0.91 | **0.41** | 0.36 | 0.14 | 0.44 |
| push_cls | 0.89 | **0.40** | 0.34 | 0.14 | 0.45 |
| cycles | 0.29 | **0.19** | 0.95 | 0.07 | 0.31 |
| churn_read | 0.02 | 0.02 | 0.35 | 0.08 | 0.30 |
| deeplist | 0.03 | 0.31 | 1.14 | 0.09 | 0.22 |

`churn_alloc` ran **13 whole-arena full collections (0.477 s of pause)** where the same program at `a853135aa` ran **105 copying minors (0.016 s)**. `tree`'s total GC pause falls **4.107 s → 0.550 s** and its max pause **266 ms → 16 ms**; `trace_worklist` falls from 2,877 ms out of the top six phases entirely. `tree_wide` lands at 0.549 s of pause, 17 ms max.

**The #7161 blocker that made polls-off a stopgap is discharged, and separately so is the reason this flip was declined once before.** A poll at every back-edge used to delete the #7480 element-shape fast clone — a call inside a clone whose admission rests on being call-free-by-construction does not slow it, it removes it. Step 4 of that work now refuses to emit a poll inside such a clone, and `churn_read` measures 0.02 s with polls either way.

**Costs, measured rather than argued.** `deeplist` 0.03 → 0.31 and `retain1` 0.03 → 0.42: both are workloads whose heap stays under the initial 64 MB threshold, so they previously ran *zero* collections and a moving nursery is pure added cost there. Both still beat `a853135aa` (1.14 / —). `push_num` 0.16 → 0.17.

Three tests pin what was unpinned. `polls_default_is_on` and codegen's `moving_safepoint_poll_default::unset_emits_the_poll` each pin one half against the full spelling table including the unrecognised-value arm, which is the one that silently changes meaning if the `matches!` is inverted back. `polls_default_matches_codegen_mirror` pins that the two crates agree — the disagreement is silent in both directions (polls nothing consumes, or a deferral nothing drains), so it needs its own assertion rather than two doc comments claiming they match.

Validation: perry-runtime 1947/0, perry-codegen lib 796/0, gap suite, `PERRY_GC_VERIFY_MARK=1` + `PERRY_GC_VERIFY_EVACUATION=1` clean on six benches, and `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` at quarantine depths 8 and 800 moving 4,837 objects with byte-identical output. Zeal's own verdict line is the liveness proof that the new default is live rather than merely untripped: `forced_collections=2000005 copying_minors=2000005 moved_objects=8 loop_polls=2000000`.

---

**`perf(gc)`: the trace path stops paying a shape-layout hash lookup per object for a disabled counter.**

`heap_payload_slot_selection` runs once per traced object per GC walk — mark, rewrite and verify. For every `GC_TYPE_OBJECT` it computed `raw_numeric_object_slots` through `with_typed_descriptor_for_query`: a per-object map probe plus, for every class instance, a `SHAPE_LAYOUTS` hash lookup behind a thread-local `RefCell` borrow. That number has exactly one consumer, `record_layout_raw_numeric_object_field_range_skipped`, which returns on its first line unless `PERRY_GC_LAYOUT_SCAN_TRACE` armed the counter. The shipped collector paid a SipHash per object to produce a number nothing read — the same shape as #7702, where a facility already disabled at runtime still had its arguments evaluated. It is now computed only when the trace is armed.

Same walk, second item: `shape_shared_pointer_mask` returned `shape_shared_descriptor(user_ptr).map(|d| d.pointer_mask)`, cloning a whole `TypedLayoutDescriptor` to keep one of its two masks. `LayoutSlotMask` is `Heap(Vec<u64>)` above 64 slots, so a traced wide object allocated and freed a second vector — the `raw_f64_mask` it immediately dropped — on every walk. It now borrows through `with_shape_shared_descriptor` and clones only the mask it returns; `shape_shared_descriptor` had no other caller and is removed rather than left as dead code.

Both are behaviour-preserving: no mask, no slot selection and no counter value changes when the layout-scan trace is armed.

---

**`test(gc)`: the alloc-point rooting tests declare the pacing they actually assert at.**

Four `runtime_roots` tests took no pacing guard and so inherited the process default, which this stack changes. They are not asserting about the default — they assert that a specific runtime helper's object survives a collection *at the allocation point*, reached through the direct alloc-point minor. Under moving-loop polls that pressure is deferred to a precise safepoint, and a Rust unit test has no loop back-edge poll to drain it.

`force_legacy_gc_pacing` is the wrong repair, and the tests say so themselves: three carry an evacuation witness — *"the minor did not evacuate, so nothing here was exercised and a green result would be meaningless"* — and legacy pacing hands the work to the budgeted stepper, which is deliberately non-moving, converting a failed assist assertion into a failed liveness assertion. `force_alloc_point_minor_pacing` (polls OFF, scavenge ON) is the one combination in which both halves hold, and is what these tests were written against. `symbol_description` has no witness and takes `force_legacy_gc_pacing`. Each records that the moving default's rooting coverage for these helpers is the gap suite's `test_gap_gc_*_rooting.ts` cases and the zeal runs, not this vehicle — so a pinned pacing is not mistaken for the default going untested.
