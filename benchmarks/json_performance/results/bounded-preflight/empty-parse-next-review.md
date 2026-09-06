Potential next experiment, not implemented or measured.

The current empty-object parser obtains the canonical zero-key shape, calls
js_object_alloc_class_inline_keys(0,0,0,keys), and marks the result plain ordinary.
A replacement must preserve fresh identity, Object.prototype behavior, ordinary
write eligibility, retention/mutation semantics and typed/reviver behavior.
Bare js_object_alloc(0,0) does not set OBJ_FLAG_PLAIN_ORDINARY, so the parse birth
site would have to call mark_object_plain_ordinary explicitly. Null keys versus
a canonical empty keys array also needs downstream compatibility validation.

Recognize the entire empty-object grammar, including only JSON whitespace and
no trailing data, before any collecting operation. Drop all input borrows, then
service the existing pending-parse hook and allocate the final object normally.
A normal allocator alone is not proof of equal RSS: the parse-specific pressure
scheduler uses a different threshold. Keep the existing pressure-scheduling hook
after allocation (it only sets debt, it does not collect) if this experiment is
pursued. Do not remove debt service, ignore outer suppression, or change the GC
production policy/thresholds. Clear oversized parse-key cache/ring metadata as
on the scalar return. Output remains live across any actual post-allocation GC.

The old suppression/rebaseline cycle can potentially be unnecessary for a
single final allocation, but that is a hypothesis requiring churn/retention,
forced movement and full CPU/RSS measurements. It is not current behavior.
