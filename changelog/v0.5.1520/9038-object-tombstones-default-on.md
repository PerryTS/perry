Tombstone object deletes (#9029) are now the DEFAULT. `delete obj[k]` on an
owned keys array is O(1) — a hole marker plus a barriered value clear —
with threshold compaction amortizing the debt (never more than 2x live
size). `bench_populated_delete` drops 2030 → ~315 ms (6.5x; ~15x node on
the same host, from ~96x before the campaign). Deletes on cache-shared
keys arrays still clone-compact once to take ownership; every later delete
on that object is O(1).

The kill switch is `PERRY_OBJECT_TOMBSTONES=0` (also `off`/`false`),
mirroring the moving-scavenge rollout's `PERRY_GC_MOVING_LOOP_POLLS=0`
pattern. `=1` remains accepted and is now redundant.

Shipping default-on was gated on the #9029 walker audit (all 57 files
touching keys arrays classified; four flag-on-only bugs found and fixed,
including a JSON-template SIGSEGV and the hole-count accounting reset that
let delete/re-add churn dodge the squeeze bound) and on the churn-bound
unit test that pins the 2x-live-size memory guarantee. Full suite runs
with the default flag, so every delete-touching test now exercises the
tombstone path; the four differentials plus the two holed-JSON-array
repros stay byte-identical to node in BOTH flag directions.
