**Gate follow-ups for train126.**

- `PASS1_MARKED`'s `non_moving_snapshot` window re-pinned after #9755
  restructured `gc/cycle.rs`. Its hunks are all root-scan machinery
  (`RootScanSubphase`, `RootScanCycleState`, the mutable-scanner iteration
  state), which runs before mark propagation completes; the bracketing is
  unchanged — `census_pass1_if_armed` inside `step_mark_propagation`,
  `census_take_if_armed_at_full_sweep_start` inside `step_sweep` — and a
  synchronous full mark-sweep still moves nothing between them.
- `TRANSITION_CACHE_YOUNG` and `SHAPE_CACHE_YOUNG` classified. Both are
  `YoungLog<u32>` remembered sets holding slot indices and shape ids — a `u32`
  cannot hold a 48-bit pointer — and the pointer-bearing entries they index are
  visited by registered scanners.
- Shape-descriptor callsite baseline updated for #9755's relocation of
  `visit_raw_mut_ptr_slot(&mut entry.keys_array)` into the new
  `object/side_table_roots.rs`.
- `regex.rs` crossed the 2000-line cap by one line, so the `replaceAll` /
  `matchAll` non-global receiver guards moved to `regex/global_guards.rs`,
  gated on `regex-engine` like their siblings.
