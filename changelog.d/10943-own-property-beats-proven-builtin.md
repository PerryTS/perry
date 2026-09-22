An own property that shadows a builtin method now beats it on a PROVEN
receiver, closing #10943 for Map, Set, Date and the array methods other than
`push`. ECMA-262 resolves `recv.m(a)` as `Get(recv, "m")` then `Call`, and
perry lowered a method call on a proven receiver straight to the native helper,
which an own property left entirely intact: `const m = new Map(); m.get = () =>
1; m.get("k")` ran `Map.prototype.get`. Reflection already disagreed with the
call -- `typeof`, `hasOwnProperty` and `Object.keys` all saw the own property.
The guard covers all four lowering layers (the method-call chain, the folded
builtin nodes, the universal dispatcher, and the declared-collection helpers),
and it is keyed on node identity so a folded call nested in an ARGUMENT --
`m1.set("k", m2.get("k"))` -- gets its own guard rather than inheriting the
outer node's suppression.

`arr.push(x)` IS STILL WRONG when the array has an own `push`: it runs the
builtin, exactly as before this change. Guarding it costs the push its inline
store -- +94 instructions per call, measured, against +6 for `indexOf` -- and
the cheap absence proof every other kind has does not exist for an array: one
that takes an own named property records nothing in its header that the inline
push tier can test. That gap is filed as its own issue; until it is closed,
`push` stays out of the gate and this fix is 36 of 37 cases, not 37.

The common case costs a flag test rather than a call: the guard performs the
runtime predicate's own first proof inline -- a header-bit test for a proven
array, a monotonic load of the install flag for every other kind -- and calls
the predicate only when that proof fails. Measured against the previous
revision: a hot `m.get(k)` +38.000 -> +8.000 instructions per call, a hot
`a.indexOf(x)` +4742 -> +7.000, with an element-read control flat.
