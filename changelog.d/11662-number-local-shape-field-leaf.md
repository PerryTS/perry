`h = h + o.a` on a shape-proven receiver `o = new C(...)` now lowers as a
Number accumulator: the #10777 shape-field leaf of the function-scope
Number-by-construction rule is unconditional, and the default-off
`PERRY_L14_NBC_ORDER` knob (and its build/object cache keys) is deleted. On the
5L `fpxnum` fixture the loop drops from 25 to 10 instructions per iteration, with
no `js_dynamic_string_or_number_add` call and no root barrier on `h`
(charter step 5L).
