### Fixed

- **Wide-object JSON parse held 3.2x the memory it needed, because nothing scheduled a full collection (#10123).**
  `JSON.parse` of a 50,000-field object, repeated, grew to 387 MiB peak RSS against a
  6.4 MB live set. The cause was a blind spot in old-gen pacing, not in the parser.

  `credit_promoted_bytes_to_old_baseline` adds a minor's promoted bytes straight onto
  the old-reclaim baseline, so `old_in_use - old_baseline` is **invariant under
  promotion** by construction. That is deliberate and still correct (#7902, #7965): the
  baseline is the base of a growth measurement, not a liveness claim, and withholding
  the credit pins that base at 0 on exactly the workloads that reach it. But it leaves
  one question unanswerable — *how much of old-gen arrived because a minor could not
  tell garbage from survivors?* On this workload every minor promoted at 999 permille,
  so the entire nursery was laundered into old-gen where no minor would ever look at it
  again, and the only thing that ever triggered a full was incidental born-tenured
  allocation elsewhere.

  Fixed by recording a second, independent signal alongside the baseline credit rather
  than correcting it: bytes promoted by a minor whose survival was >= 950 permille
  accumulate in `GC_OLD_GARBAGE_SUSPECT_BYTES`, and 16 MB of that makes an old reclaim
  due on its own. The baseline is untouched, so both #7902's and #7965's arguments still
  hold. The floor is the measured knee of a 2/4/8/16/32/64/128 MB sweep.

  Peak RSS on `wide_1m:parse` at 256 iterations: **387 MiB -> 243 MiB (-37%)**, at
  baseline CPU (3.00 s vs 3.03 s unpatched, best-of-3 interleaved).

  Two findings from the same investigation are recorded on #10123 rather than fixed
  here, because each is independent and one is a trap:

  - Removing the garbage at its source makes things **worse** on its own. `JSON.parse`
    re-mints every key of a wide object as a born-tenured string each call, because the
    parse-key cache holds 4096 entries and a 50k-field object overflows it on the first
    parse, so the boundary clear wipes it before the second parse can hit it. Raising
    that bound does remove the churn — and thereby removes the *only* thing that was
    scheduling fulls, taking peak RSS from 387 MiB to 833 MiB with zero full
    collections. It must not land without pacing that survives it. With this change in
    place it is measurably redundant (243 MiB either way), so it is not included.
  - RSS now floors at 243 MiB no matter how many fulls run, because freed pages are
    never returned: `arena_live` is 56.7 MB against a 67 MB arena, while the process
    holds 243 MiB, and block release reports `examined=41 released=0 no_snapshot=40`.
    That is the next lever on this row and is independent of pacing.
