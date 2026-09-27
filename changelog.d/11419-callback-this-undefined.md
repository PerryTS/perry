Fixed `sort` comparators and `reduce` / `reduceRight` callbacks seeing the
enclosing method's receiver as `this` (#11419). The runtime invoked them
through plain `js_closure_callN` calls without resetting the per-thread
implicit-`this` cell, so a comparator called from inside `obj.m()` read `obj`
instead of `undefined`. The `forEach` / `map` / `filter` / `find*` / `some` /
`every` / `flatMap` family already bound `undefined` around their callbacks;
the sort and reduce entry points now do the same through a new
`ImplicitThisScope::bind_undefined`, covering arrays, typed arrays (including
BigInt lanes and `toSorted`) and generic array-likes
(`Array.prototype.sort/reduce/reduceRight.call(obj, …)`). The guard restores
the caller's receiver from a rooted slot on return and on unwind.

The structural fix (receiver as a calling-convention parameter, deleting the
cell) remains the `this`-as-parameter plan; this closes the observable leak
for these callbacks. Regression test:
`test-files/test_gap_11419_callback_this_undefined.ts` (every
surface leaks on `main`; all pass with this change).
