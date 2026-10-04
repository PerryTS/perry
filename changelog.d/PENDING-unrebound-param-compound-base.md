`a[k] op= v` whose receiver is a parameter that nothing in its function can
rebind (no assignment, update, `var` or function re-declaration, and no
`arguments` or `eval` anywhere in the function) now writes through the
parameter itself instead of a `__cmpd_base` copy, as a `const` receiver
already did. The copy cost a GC root slot, a string-addref test and a
root-shading gate per statement; the #11810 n-body kernel drops from 592M to
237M instructions.
