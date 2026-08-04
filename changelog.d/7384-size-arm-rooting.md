**Fixed** three stale-receiver sites in `js_object_get_field_by_name`'s Map/Set
subclass `.size` arm.

The arm calls three helpers that allocate, in sequence, with the receiver held in
a bare local across all of them:

- `own_key_present` — inside the original `if` condition
- `class_instance_has_member` — builds a `String` for its cache probe
- `subclass_backing_of` — calls `js_string_from_bytes` to materialise its
  constant `BACKING_KEY` on *every* call

Any of the three can drive an evacuating minor, and on the `None` fall-through
every later arm of the function dereferences the receiver again. The faults are
the GC-type probes at `js_object_get_field_by_name` +560 and +664, reached after
the plausibility checks pass because a retired from-space address still looks
like a plausible heap pointer.

The scope is opened only after the key is confirmed to be `"size"`, so ordinary
property reads are untouched — this arm is not the general fast lane, and the
`RuntimeHandleScope` must not land on one.

**This does not close `test_gap_field_lane_semantics` or
`test_gap_put_value_plan_cache`.** Each fix moves the fault to the next site in
the same function (+664 → +560 → +820), so at least a fourth remains. The three
closed here are real — a no-op leaves the faulting offset byte-identical — but
the function is a chain, not a single defect. 58 pass / 2 fail on the
object/assign/class/field/shape gap set, byte-identical to pristine `main`.
