**The per-minor dirty-scan covered set is pre-sized instead of being rebuilt
from empty**, removing the hashbrown growth ladder every copying minor walked.

`dirty_scan_covered` is created with `new_ptr_hash_set()` at the top of every
copying minor and filled during the dirty-slot scan. Measured with
`[gc-dirty-covered]` (added here), it reaches **~119,000 entries** on a
3300-character claude-code reply — not the ~1,000 the `[gc-restore-coverage]`
`objects_skipped` figure suggested — so it walked hashbrown's capacity ladder
(1,792 → 14,336 → 57,344 → 114,688 → 229,376) and paid a
`RawTable::reserve_rehash` at each boundary, re-hashing and re-copying the whole
table. `reserve_rehash` was **217 leaf samples, 1.49 % of the turn**, 111 of
them under `PtrHashSet::insert` and the rest under `run_copied_minor_attempt`
and `restore_surviving_dirty_coverage`.

The set is now pre-sized from the previous minor's count, the same treatment and
the same justification as `PREVIOUS_SURVIVOR_ESTIMATE` immediately above it: the
count is autocorrelated between adjacent cycles, over-estimating costs only
untouched reserved bytes, under-estimating falls back to ordinary growth, and
the estimate shares that constant's cap so one huge cycle cannot make every
later cycle reserve unboundedly.

`reserve_rehash` falls **217 → 167 leaf samples (1.49 % → 1.24 % of the turn)**.
The rig is flat, as expected of a 1.5 % item — 400-character turn CPU 4.05 min
against 4.14, 3300 17.86 against 17.71 — with settled footprint and peak RSS
improving at 400 (557 → 457 MB, 604 → 563 MB) and flat at 3300. The ground
claimed is **work permanently removed**, counted rather than inferred:
`[gc-dirty-covered]` reports `len`, `capacity` and `presized_to` per minor, so
the pre-size can be seen tracking rather than assumed to.

**A high-water estimate was tried and rejected.** It is better on the mechanism
— under-shoots fall from 57 of 96 minors to 21 of 97 — but reserving the peak on
every minor cost settled footprint 763 → 1165 MB and peak RSS 974 → 1250 MB at
3300 characters for no measurable time difference (167 vs 182 leaf samples,
inside run-to-run noise). Trading footprint for CPU is rejected, and here it did
not even buy CPU. The rejection is recorded at the function so the next person
does not re-derive it.
