**ci(compiler-output-regression):** the `native-region-proof` gate accounts for
#7088's inline shadow-slot root barrier, restoring green on `main`.

#7088 moved the per-store shadow-stack root store — and its
incremental-mark root-shading barrier — from a `js_shadow_slot_bind` /
`js_shadow_slot_set` runtime call to inline IR. That barrier was always
emitted; it just lived *inside* the runtime function, invisible to the
harness's static call counter. Inlining made the `js_write_barrier_root_nanbox`
call site visible, so `write_barriers_static` jumped (e.g. h1_native_rep_equivalence
0→3, one per rooted Buffer local) and every affected `native-region-proof`
workload tripped its heap-barrier budget. The same inline lowering inserts
`ss.*` blocks ahead of the module-init loops, shifting the deterministic
per-function block counter by 12 and blanking the `direct_bounded` /
`local_cast` / `helper_index` region labels (`for.body.2/6/10` → `14/18/22`).

Neither is a real regression: the root-shading barrier is gated behind
`PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`, never fires in these workloads
(`write_barriers_traced` stays 0), and #7088 proves it observationally
identical to the call it replaced. The barriers sit in guarded `ss.barrier`
blocks at root-bind sites, never inside the native loops, which still carry
raw `load i8`/`store i8` with alias metadata and no runtime calls.

- `structural_counters` now scores `write_barriers_static` on the
  optimizer-controlled *heap* barriers (`js_write_barrier`,
  `js_write_barrier_slot`) only. The shadow-stack root-shading barriers
  (`js_write_barrier_root_nanbox`, `js_write_barrier_root_heap_word`) are
  reported under a new `root_shading_barriers_static` field — still visible,
  no longer inflating the heap-barrier budget. Real regressions stay caught:
  heap barriers are still counted, and a root barrier that actually *fires*
  is caught by the `write_barriers_traced` budget.
- `h1_native_rep_equivalence`'s region selectors follow the renumbered loop
  bodies (`for.body.14/18/22`).
