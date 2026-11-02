Whole-program and per-unit facts give a `length` read of a real Array or a
primitive String the verdict OwnLength: the property is own and
non-configurable on those values, so the read needs no prototype check. A
value that may be no Array (the arguments object, an array made through a
written constructor or species, a container of primitives) never gets it. Proven
sites rise on tssrc (20,139 to 20,427 per unit, 22,016 to 22,239 whole) and the
private chess witness (523 to 534 per unit, 526 to 540 whole).
