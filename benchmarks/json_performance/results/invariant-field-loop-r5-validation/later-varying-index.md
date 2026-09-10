# Investigation after R5 measurements (not implemented or selected)

The remaining random/modulo and mixed-field controls cannot inherit the literal
index hoist: every iteration names a different record, and field values/getters
can differ. Existing element_shape_loop proof excludes class0 JSON because its
fields are boxed JSValues, not declared raw-f64 layout slots.

A bounded ordinary-array clone could hoist receiver branding and a scalar
(shape id, own slot index) candidate, then keep per-element brand/shape/descriptor/
bounds/numeric checks with an exact side exit before the current addition. Slots
are unboxed only after real numeric guards. No managed-pointer cache; borrow an
array base only for an independently certified call-free clone. A metadata probe
must not allocate a heap key or invoke a getter. On side exit write back current
sum/counter before the existing body re-executes the current iteration.

Pristine lazy arrays need a separate proof: a scalar memo may be absent or belong
to another field, and an exposed record always wins. A cached-length/shape proof
for an ordinary backing says nothing about a pristine tape header. Do not force
materialization merely to enter the clone: that would erase the measured parse/
scan RSS gains. Populating an existing pointer-free numeric memo range is a
possible later experiment, but must price its whole prepass against actual use,
preserve all exposed records and exact chosen-key identity, and be rejected for
zero/few-trip or wide/mixed workloads when unprofitable. No cap change based only
on parse timing; full consumption and retained-memory rows remain required.

Do not choose any of these until R5 correctness, emitted machine code, qualified
access/focus CPU and RSS results are available. No performance claim here.
