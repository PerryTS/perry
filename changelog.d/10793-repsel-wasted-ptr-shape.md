**A `Ptr<Shape>` proof that has no access site to spend now says so, instead of reading like an honest zero.** Fixes the `repsel-census` gate, red on `main` since v0.5.1613.

The census refused a workload with a wasted promotion and no named mechanism:

```
batch: 1 selected ptr-shape promotion(s) were not consumed, and not one of them
names a mechanism. … Either a mechanism recorder was removed, or a new way to
drop a proof exists and needs one.
```

The wasted promotion is `row` in `buildRows` (`benchmarks/app-patterns/kernels/batch.ts`). Its only use is `rows.push(row)`, a contained element push, so it survives rule 2 with **no property access anywhere in the body** — the proof is sound and there is nothing to spend it on. That is the residue `check_unconsumed_is_explained`'s docstring already described as "dropped by nobody" and then tolerated, on condition that some *other* promotion in the same workload named a rule. Nobody is a mechanism; it just had no recorder.

#10769 exposed it without causing it. Confirmed by A/B rather than inferred: subtracting only #10769's `RepselContextFlags::derive` `Entry` hunk from today's `main` reproduces the v0.5.1612 `batch` report entry-for-entry and the census goes green. What that hunk changed is `batch`'s **other** wasted promotion — `totals`, in the module-init body, previously dropped by `module_init_context` and now consumed. Removing the only named mechanism on the workload left the pre-existing residue as the only wasted promotion, and therefore the only one the check could see.

So the fix names the mechanism rather than undoing anything. No floor was lowered and no allowance widened.

- `PTR_SHAPE_NO_ACCESS_SITE` (`no_access_site`) joins `module_init_context` and `scalar_replaced` in `expr/slot_rep.rs`. The three are disjoint by construction: `module_init_context` fires from `ptr_shape_receiver_fact`, which only runs *at* an access site, and `scalar_replaced` fires for objects whose HIR field accesses are exactly what scalar replacement rewrites.
- `ptr_shape_report::note_no_access_site` records it, called from the same loop iteration as the `select()` it annuls. The verdict reads the use walk's **own** containment bookkeeping — `field_stores`, `method_calls`, plus a new `UseWalk::field_reads` for the one shape that stores nothing — rather than a second, separately-drifting definition of "access site". Report-only; `--opt-report` off allocates and records nothing.
- Inferring the rule at render time from `selected − consumed` was rejected: it would name a mechanism for *every* gap and permanently disarm the check.
- `report_early_bail` moved to `ptr_shape_report.rs` as `report::early_bail` (it calls nothing but `report::`) to keep `ptr_shape.rs` under the 2000-line cap.

Effect on the corpus is one report entry: the full 29-workload run against the v0.5.1616 release compiler differs from the fixed compiler by `no_access_site 1` and nothing else — every promotion count, floor and advisory-improvement line is byte-identical.

The gate still discriminates, checked in both directions. Disabling the new recorder returns `batch` to the original failure; disabling the pre-existing `scalar_replaced` recorder reddens `fixture_alloc_buckets`, `suite_07_object_create` and `suite_12_binary_trees`, which `no_access_site` does **not** paper over. `a_selected_promotion_with_no_access_site_names_its_mechanism` asserts both halves and fails under either sabotage — including a recorder that fires on a local that *does* have an access site, the direction that would turn the census permanently green.
