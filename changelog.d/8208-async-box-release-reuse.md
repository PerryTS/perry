**fix(async): completed plain-async activations release and reuse their box cells, removing the ~500 B/request malloc-side accumulation** (#7933 follow-up)

Plain-async activations box body locals and state-machine control cells in 8-byte `std::alloc` cells. #7933 stopped completed activations from retaining the JS values in releasable cells, but the cells and registry entries themselves remained malloc-resident for the process lifetime. That produced linear Rust-side growth which was invisible to every GC counter.

The release is now real:

- New HIR `Stmt::ReleaseBoxes(ids)` carries the terminal reclamation set through id-remapping transforms and lowers to the typed `js_box_release`, `js_i32_box_release`, and `js_bool_box_release` helpers.
- Release clears and de-registers a cell, evicts its positive-cache entry, and parks its address in the owning activation's tagged release range.
- A stable malloc-side activation token owns one lifecycle reference plus one reference for every queued or running `Task::AsyncStep`. Only the zero-reference transition publishes that activation's cells to the per-kind intrusive free lists.
- Pending-await thunks capture the stable token pointer plus its generation, rejecting stale captures after token reuse without a process-growing lookup table. The execution-reference stack explicitly releases the current pump's ownership across `longjmp`.
- Untracked runtime releases retain the old whole-pump quarantine as a conservative fallback.

Cell memory is deliberately never returned to the allocator. An address minted as a box remains readable box-cell memory for the life of the thread, preserving #4898's pointer rejection and #7906's positive-cache invariant.

The per-activation boundary is required for correctness. A stray resume writes `__gen_sent` before the `__gen_done` check short-circuits, so terminal state alone cannot make a body-local cell reusable: that write could otherwise land in a cell already registered to another activation. Publication at activation reachability zero makes the stale write unreachable by construction. Closure-visible locals remain excluded from terminal release.

Matched static-runtime measurements, peak RSS best-of-5 with byte-identical stdout:

| BATCHES | base RSS | activation reachability | final vs base |
|--:|--:|--:|--:|
| 30 | 21.625 MiB | **21.547 MiB** | **−0.078 MiB** |
| 60 | 28.109 MiB | **27.922 MiB** | **−0.188 MiB** |

The small-workload RSS floor is closed. Resident cells are constant at **1,635** at both sizes: BATCHES=30 reports 48,238 allocations / 48,238 releases / 46,603 reuses, and BATCHES=60 reports 96,448 / 96,448 / 94,813. At BATCHES=1200, reachability accounting costs +2.38% instructions relative to the earlier pump-quarantine implementation while leaving the complete change **−8.98% instructions versus base**; peak RSS is 79.17 MiB.

The former continuous-cascade limitation is also closed: the exit-path fixture reuses 19,997 of 20,027 released cells without waiting for the global task queue to empty. That fixture covers normal return, throw/rejection, early return, `try`/`finally`, async-generator termination, and loop-created closures retained across a real queue drain and a later activation allocation.

Regression coverage includes ten focused runtime release/reachability tests, terminal-arm transform coverage, typed codegen lowering tests, static GC/TLS/dominance gates, and byte-identical parity for the exit-path fixture.
