Retargeted `scripts/shape_descriptor_census.py`'s generic-read-PIC kind proof
from an **instruction** to the **invariant** that licenses its removal.

The census required the emitted fragment

    cond_br(&is_plain_kind, &tok_label, &cold_label)

in `lower_generic_property_get` — the `obj_type == GC_TYPE_OBJECT` fence in
front of the ShapeId compare. #10843 (`810fcd1ecc`, "take the GC-header load
off every generic property read") deletes it, and the census refused the tree.

**It was right to refuse, and the PR is right to remove it.** The fence existed
because a non-object cell whose payload `+4` word happened to land in
`[SHAPE_ID_BASE, SHAPE_ID_END)` could otherwise be read as a shaped object.
#10828 closed rule 3 — *no pointer-tagged non-object cell holds a ShapeId-range
value at payload `+4`* — by bounding every count-carrying kind in **release** at
its allocation and growth funnel and relaying out `DateCell`, whose `f64`
timestamp had no bound to appeal to. A successful shape compare therefore
proves the kind by itself, and the header load is redundant. Under the new
object model, asserting that branch asserts nothing about safety — only that a
dead load survived.

So the census now asserts the licence, in `object/shape_rule3.rs`:

    no row of RULE3_KINDS carries the verdict Rule3Word::RangeReachable

which is the file's own statement of "no kind keeps the GC-kind fence alive",
and the same set the runtime test `rule3_no_kind_still_requires_the_gc_kind_fence`
pins. That is strictly stronger than the line it replaces on the axis that can
actually regress: the branch could only ever notice its own deletion, whereas
this goes red the moment a **new** GC kind's `+4` word can alias a live ShapeId
— precisely the condition under which the removed load would be needed again —
and it names the kind rather than an instruction.

Three things guard it against passing vacuously, because "no row is
`RangeReachable`" is also true of a table that no longer exists (#7024/#7025's
failure mode — a green gate whose subject never ran):

* the kind set is re-derived from `gc/types.rs` and every declared `GC_TYPE_*`
  must still carry a verdict, so a dropped row is red, not silently licensed;
* a row naming a kind that no longer exists is red, so a stale table cannot
  stand in for a live one;
* the `RangeReachable` variant and the runtime test that walks the table must
  both still exist, so the property cannot be satisfied by deleting the concept.

The census's **sabotage self-tests** were retargeted with it, two arms: one
plants `Rule3Word::RangeReachable` on `DateCell` — the one kind #10828 could
not bound and had to relayout, so it is the real historical hole rather than an
invented one — and the other deletes that row entirely, proving the coverage
half catches an emptied table. Verified independently of the self-test by
reopening rule 3 in the real source on a **different** kind (`GC_TYPE_MAP`),
which exits 1 with

    rule 3 is REOPENED by GC_TYPE_MAP: shape_rule3.rs classifies the payload +4
    word Rule3Word::RangeReachable, so a pointer-tagged cell of that kind can
    carry a live ShapeId and a shape compare alone no longer proves the
    receiver is a GC_TYPE_OBJECT. The generic read PIC dropped its GC-kind
    fence on the promise that no kind can (#10828/#10843) -- restore the fence,
    or bound the kind's +4 word below SHAPE_ID_BASE

and passes again once restored.

`crates/perry-runtime/src/object/shape_rule3.rs` joins the census's
`authority_paths`, so the file going missing is itself a census failure rather
than a check that quietly stops running.

**Scope note.** The removed branch also happened to keep a STRING-tagged
receiver off the shape compare. Rule 3's guarantee is stated over
*pointer*-tagged values, so that half of the licence is the emitted receiver
test being the exact `POINTER_TAG` test rather than the collapsed
pointer-or-string one. It is not part of this assertion: #10843's
`generic_non_length_read_keeps_the_whole_tower` pins it in
`perry-codegen/src/expr/property_get/tests.rs` against the emitted LLVM IR,
which is a stronger instrument than a source-text regex in the census.
