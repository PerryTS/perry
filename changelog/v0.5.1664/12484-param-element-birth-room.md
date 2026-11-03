Literal births reserve room for keys added through parameters and array
elements (#12484, phase 2 of #11497). The capacity evidence that sizes a
literal's birth now follows a literal into the parameter of a module-local
function or closure it is passed to, and into an array it is pushed onto (or
listed in) and read back from by index (`a[i]`, `a.at(i)`). A key the
consumer adds is then an inline slot, so the store and the read after it take
the emitted shape compare and one load instead of the out-of-line spill store
and spill read. A reassigned parameter or array binding fails closed. This is
capacity only (still capped at eight extra slots): keys stay authoritative and
no access assumes anything from it.

`addkey` param x lit/factory: 356 -> ~57 instructions per iteration (num),
483 -> ~112 (str); varying x lit/factory: 397 -> ~60 (num), 528 -> ~115
(str). See the lane report for the measured values.

Tests: `born_wide_parameter_evidence`, `born_wide_reassigned_parameter_fails_closed`,
`born_wide_array_element_evidence`, and `test_gap_12484_param_element_birth_room`
(key order, enumeration, JSON, `in`, delete and re-add, more added keys than
the cap, element reads, a frozen literal).
