Reserve up to eight inline slots for named keys added later to literal objects
(#11497, #10496), using the existing class birth images and allocation paths.
Capacity evidence covers unreassigned local, captured and module bindings,
module-local factories with return-shape facts, and `this` in literal methods.
Parameters and array elements remain deferred to phase 2. Reserved capacity
never publishes a property or permits an unguarded access.

One shared slack calculation controls allocation, module birth images and
constant literal descriptors. Classes sharing a keys global use its maximum
requested capacity. ConstFn final shapes preserve the capacity, keeping the
existing literal-method specialization. The descriptor width uses existing
padding on 64-bit targets, preserving its 64-byte record size. Tests cover birth images, allocation widths, precision, the eight-slot cap,
descriptors, enumeration and moving GC.

Validation and measurements are recorded in
`docs/performance/11497-literal-key-room.txt`, including the scalar spill
layout-note investigation. No layout-note correctness fix was needed: the old
bits passed to the helper matched the previous stored value.
