Advanced the `PASS1_MARKED` window pin in `scripts/gc_runtime_root_holders.json`
after #10834's one-line change to `crates/perry-runtime/src/gc/mod.rs`, with the
re-audit that pin exists to force.

`PASS1_MARKED` is the census's mark-phase snapshot: a `Vec<usize>` of real GC
header addresses, deliberately untraced so the diagnostic does not keep its own
subjects alive. It is populated by `census_pass1_if_armed` at the end of mark
propagation and consumed by `census_take_if_armed_at_full_sweep_start` at sweep
entry, within one `run_to_completion`. The contract is that nothing in between
allocates a GC object, relocates, collects, or runs a JS callback — and the gate
pins the sha256 of the five files that could break it, so a change to any of them
fails until someone writes down why the window still holds.

#10834 touches two of the relevant places, and both sit **outside** the window,
on opposite sides of it:

* `gc/mod.rs` gains exactly one line — a `reg_scanner!` for the inherited-read
  cache's root scanner. Mutable-root scanners are consumed inside
  `RootScanCycleState::step_current_subphase`, entirely within the RootScan
  phase: `step_root_scan` only advances to `GcCyclePhase::MarkPropagation` once
  that loop reports done (`gc/cycle.rs:958-961`), and `census_pass1_if_armed()`
  fires at the *end* of `step_mark_propagation` (`gc/cycle.rs:982`). The scanner
  runs strictly before the window opens. Its body is a bounded walk of a fixed
  512-entry thread-local calling `visit_tagged_usize_slot` /
  `visit_usize_slot` — no allocation, no relocation, no JS. This is the same
  shape already cleared for #9769, #9976/#9977, #10054, #10055 and #10735.

* `gc/dead_owner.rs` (not a pinned source) gains an `INHERITED_READ_CACHE` entry
  in `DEAD_KEY_PRUNES`. That registry is consumed by
  `IncrementalSweepState::with_dead_collection_finalize` at `gc/cycle.rs:1548` —
  *after* `census_take_if_armed_at_full_sweep_start` at `gc/cycle.rs:1505` has
  already `take()`n the snapshot out of the thread-local. Same argument that
  cleared #9845's `collect_dead_registered_regexps_post_trace`.

Verified that `gc/mod.rs` is the **only** pinned source whose digest moved before
advancing it; the other four were re-hashed and asserted unchanged, so this
re-audit is not silently covering a second change. The note is appended to the
entry's existing audit trail rather than replacing it, and the edit is two lines
— the file was not reformatted.
