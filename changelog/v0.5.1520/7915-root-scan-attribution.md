### Added

- **gc: per-scanner root attribution for the copied-minor root scan (#7915).**
  Under the existing `PERRY_GC_DIAG` (no new knob — CLAUDE.md's GC knob
  kill-policy), each copying minor prints `[gc-scanner-profile]`: per registered
  scanner, the wall time and the slots / pointer-roots / rewrites it accounted
  for, sorted by time. Registration sites carry their own path as the name
  (`stringify!` in `gc_init`'s `reg_scanner!` / `reg_budgeted_scanner!` macros),
  so a name cannot drift from the list it describes.

  The aggregate `root_sources.runtime_mutable_scanners` counter sums all 82
  scanners and every pass into one number, which is enough to see that a cost is
  per-root and not enough to say which registry holds the roots. That gap is why
  #7915 read as a property of "82 registered scanners over 218 455 pointer
  roots" when the attribution says it is **one** scanner:
  `crate::box::scan_box_roots_mut` is 92 % of the pointer roots and 89.5 % of all
  root-scan time, while the suspected promise registry visits **12 slots** and
  costs 4–43 µs.

  The instrument then refutes the framing it was built to confirm. A cheap slot
  visit costs ~4 ns (`intern_table_mutable_root_scanner`: 16 384 slots, 1 144
  pointer roots, 68 µs); the box scanner costs 98.8 ns/slot because essentially
  every box slot *is* a from-space pointer, so visiting it in `CopyingMark` mode
  evacuates the object it names — 93 380 of one minor's 156 236 evacuations come
  straight out of box roots. The fixed per-root overhead the issue targets is
  ~4 ns × 265 000 ≈ **1 ms of a 65–101 ms pause**. Full write-up, including the
  measurement that shows `asyncpipe`'s young generation is 96 % retained by the
  never-freed box registry rather than by the program:
  `gc-handoff/ROOTS-NOTES.md`.

- **gc: counters for the three unobserved `gc_safepoint_moving_minor` entry
  guards (#7915).** Only the `budgeted` arm was counted, so a precise safepoint
  that returned without collecting had one observable explanation and three
  invisible ones. `[gc-incremental]` now also reports
  `safepoints_blocked(in_alloc=… unsafe_zone=… root_lock=…)`, which is what turns
  "an active `setjmp`/`try` region suppresses the copying minor" (#7910) into a
  claim a single run can settle.

### Fixed

- **lint: `gc_runtime_root_holders.py` did not know `gc_init`'s registration
  macros (#7915).** `reg_scanner!` / `reg_budgeted_scanner!` expand to
  `gc_register_*` calls, but the gate matches the call *name*, so introducing
  them dropped its registered-scanner count from 122 to 24 and every holder
  reached only from `gc_init` would have read as uncovered. Its `MIN_REGISTERED`
  floor caught it, which is exactly what that floor is for.
