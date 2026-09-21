Fixed numeric provenance being computed before the `Ptr<Shape>` receiver proofs
it depends on (refs #10777).

`collectors/hir_facts.rs` computed `number_by_construction_locals` before
`collect_shape_proven_ptr_locals`. For `h = h + o.a` that asks "is `h`
Number-producing?" before `o`'s receiver proof exists, and
`expr_numeric_by_construction`'s `PropertyGet` arm is gated on the receiver
being a tracked member — so the function-scope entry point passed
`empty_members`/`empty_fields` hardcoded (`ptr_shape_numeric.rs`) and that arm
could never fire. The accumulator was therefore never admitted, however
completely the receiver's shape was proven: on a fixture whose opt report says
`Ptr<Shape> 1 selected / 1 CONSUMED`, the `+` routing decision still reported
`both_numeric=false => GUARDED`.

The computation now runs after the receiver proofs, and the two hardcoded-empty
parameters become real. No new admission arm, no new provenance class, no new
fact. The shape inputs are the **intersection** of the proven receivers' numeric
field sets — the arm consumes one set and does not re-check which receiver a
property belongs to, so the set must be numeric on every admitted receiver; a
union would be a wrong answer, not a weaker one.

Default OFF behind `PERRY_L14_NBC_ORDER=1` and keyed into the object cache; with
it off the inputs are empty and the fixpoint computes exactly what it computed
before, so the reorder is a no-op.
