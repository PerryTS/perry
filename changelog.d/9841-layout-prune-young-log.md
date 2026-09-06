**A minor's per-object layout death-prune now walks a young-entry log instead
of both tables** — on a compiled claude-code streamed reply it visited
**6,792,375 entries where at most 125,367 (1.85 %) could possibly have died,
and on 37 % of minors nothing could have died at all.**

`prune_dead_per_object_layout_owners` asks "which owners died?" of every key
in `LAYOUT_SLOT_MASKS` + `TYPED_LAYOUTS`, tables sized by everything the
program ever created (~66k live keys on cc, from a history far larger). But
both of a minor's deadness predicates require the owner to be in the nursery:
`owner_is_dead_copied_minor_from_space` demands eden or the active survivor
half, and `PostTraceProbe::owner_is_dead` on a minor demands an in-arena,
untenured `HeapGeneration::Nursery` address. An owner that was old at the last
prune is still old, so the walk over it cannot remove anything.

So the two maps get the young-entry log of #9754 (`gc/young_log.rs`): every
writer notes a key whose owner `layout_key_may_be_nursery` admits before the
entry becomes findable, a minor prunes from the log, and a survivor is
re-logged only while it is still young — a promoted owner leaves the log and
no later minor visits it again. A full prune keeps its whole-table walk (old
owners do die in a full trace) and rebuilds the log from the survivors it is
already classifying, at no extra pass.

**Why this table pays where the scanners of #9754 did not.** Read back per
table on an unmodified binary, that PR's four converted tables are a net 0.78x
on cc and `closure.dynamic_props` is a 2.56x regression, because a scanner
keeps `addr_is_minor_relevant` — true for `Longlived` **by design**, since a
longlived object can point at a young one — and cc allocates its shape-key
arrays longlived, so those logs never drain (`kept/logged` median 1.000). A
prune's predicate is `layout_key_may_be_nursery`, which excludes `Longlived`
**and** `Old`; cc's tenuring promotes every survivor after one survival, so a
key leaves this log after one minor. Same mechanism, opposite sign, decided
entirely by which predicate the walk keeps on. The measured over-visit is 54x
at a 3300-character reply and 25x at 400, with `dead <= young_before` on
152/152 minor prunes — the empirical proof that the log's predicate is a sound
superset of what a minor can kill.

Rule 2 of the design travels with it: under `debug_assertions` the young prune
re-derives the candidate set from the authoritative maps and panics on any
young key the log does not name, so deleting an arming site is a red test
rather than a dead owner's record surviving in silence. The in-borrow mask
mint in `layout_note_slot` — the dominant insert path on cc, and the one site
that published a young record without counting it — is armed for the first
time here.
