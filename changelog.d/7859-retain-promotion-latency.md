### Fixed

- **A process-global `Mutex<HashMap>` was taken once per TRACED object, in a program that
  never records a prototype.** `visit_object_static_prototype_slot_mut` is the collector's
  per-object rewrite hook (`visit_gc_rewrite_slot_descriptors`'s `Object` arm), and it locked
  `OBJECT_PROTOTYPES` and ran a SipHash `HashMap::remove` before testing anything. The
  monotone latch that answers it — `OBJECT_PROTOTYPES_NONEMPTY`, stored `Release` under the
  same mutex as the only insert — already existed and was already consulted by its three
  siblings, including `object_static_prototype_owner_moved`, whose own comment names
  `gc-handoff/bench/retain.ts` and the `pthread_mutex_lock` + `RandomState` profile symbols.
  The collector's hook was the one that missed it. On a retained-live-set workload
  (`retain.ts`, 2.36 M objects traced across five minors) the map is empty for the entire run
  and the mutex plus hash was pure loss; it measured 3.7–7.0% of the program in symbolicated
  profiles. The cost scales with SURVIVORS, not allocations, so churn-shaped programs see
  ~nothing — this is the same workload dependence measured on the symbol-registry latch.
  A new unit test asserts both halves: the hook skips an empty registry, and still reaches a
  recorded entry.

### Performance

- **The copying minor's per-promoted-object cost is DRAM latency, and the loops now prefetch
  for it.** On a fully-live nursery every survivor's `GcHeader` is touched by three passes in
  different orders — the remembered-set dirty scan, the mark drain, and `clear_marks` — over a
  cohort far larger than any cache, so each touch is a full memory round trip. All three loops
  know their next target several iterations ahead (a `Vec<*mut GcHeader>`, or a contiguous slot
  range whose values decode to the addresses about to be classified), so `gc::prefetch` issues
  a non-faulting `prfm pldl1keep` / `_mm_prefetch` eight entries ahead. Faultlessness is what
  makes it usable on a *candidate* address, where a speculative load would be a
  use-after-free.

- **`CopyingNurseryCollector::mark_addr` memoizes its last successful classification.**
  `mark_addr` is idempotent with a stable result for the whole cycle, so replaying the previous
  answer is exact rather than approximate. Shape-shared children are the same address for every
  instance, so the mark drain was classifying ONE `keys_array` pointer ~750 k times per cycle,
  each a page-map lookup plus a cold `plausible_gc_header` read.

- **The remembered-set dirty scan's inner loop lost three per-slot costs.**
  `is_weak_target_trace_slot` asks a question about the PARENT — only three class ids can own a
  weak slot — so it is now decided once per descriptor via `header_may_hold_weak_target_slots`
  instead of once per slot. `old_page_account_dirty_slot` maintains a per-PAGE counter through
  a hash map while the scan walks ascending contiguous slots, so ~512 consecutive slots now
  share one probe (`old_page_account_dirty_slots`). And `layout_scan_trace_active()` — read once
  per traced object and once per pointer slot, and an out-of-line `_tlv_get_addr` on Darwin —
  is now gated on a process-global `AtomicBool`, the #7834 `PERRY_PER_OBJECT_LAYOUTS_ANY`
  pattern.

- **Array-growth barrier replay is page-batched.** `replay_array_growth_write_barriers` is the
  one barrier caller that is a LOOP over a single parent, so the incremental-shading decision
  and both parent classifications are loop invariants; and because the remembered set is page
  granular, once a page has been dirtied the remaining ~511 slots on it re-assert a fact that is
  already true and are skipped. `retain.ts` grows one array to 3 M elements, so its geometric
  growth replayed ~6 M barriers — 2.6× the cost of the `memcpy` they follow, and 9–11% of the
  whole program.
