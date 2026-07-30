### GC: whole-heap from-space scan (`PERRY_GC_FROMSPACE_SCAN`)

Adds a debug-only verification pass that answers, after the copying minor's
rewrite pass and before from-space is reset:

> does any live object OUTSIDE from-space still contain a word that decodes to a
> from-space address?

It walks the whole heap — the arena census plus the malloc-tracked registry —
and decodes every payload word through `decode_root_word`, the decoder the mark
and rewrite paths already share (#6910). Objects that are themselves in
from-space are skipped: a dead object legitimately still points at its dead
peers.

**Why this is not redundant with `PERRY_GC_VERIFY_EVACUATION` (#7035).** The
existing verifier walks the same surfaces the *rewrite pass* walks, so it can
only check that the rewrite pass agreed with itself; a holder the rewrite pass
never enumerated is invisible to both. On the #7022 reproducer the verifier
reports clean while the program dies of malloc-heap corruption. This pass does
not depend on any root enumeration, and on that same reproducer it names the
offending holders on the second collection.

Each offender is classified along two axes that have different fixes:

- **missing rewrite** (target carries `GC_FLAG_FORWARDED` — it moved and this
  reference was not updated) vs **dangling** (target was never evacuated and is
  about to be recycled);
- remembered-set coverage of the slot's page — `never_dirty` (a store path
  skipped the write barrier), `lost_dirty` (the edge was recorded and then
  lost), `dirty_but_missed` (the page is dirty and the slot was still missed).
  This reuses the existing `EVER_DIRTY_OLD_PAGES` tracking, which is now enabled
  by this scan as well as by `PERRY_GC_VERIFY_EVACUATION`.

`PERRY_GC_FROMSPACE_SCAN_ABORT=1` additionally aborts on the first offender so a
crash report captures the offending cycle rather than the downstream corruption.

O(live heap) per collection, so it is strictly opt-in and off by default.

Three unit tests cover both directions — that the scan finds a planted
un-rewritten old→young reference, that it stops reporting once the reference is
removed, that it separates dangling from missing-rewrite, and that it ignores
references held *by* from-space objects. Teeth verified by sabotage: blinding
the from-space predicate turns two of the three red with the exact
"planted=0" signature.
