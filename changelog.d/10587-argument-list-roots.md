Fixed three GC rooting gaps in #10532's dynamic-call argument-list handling:
`Reflect.apply` held the callee, receiver and arguments in plain Rust locals
across the one closure-rebind shape that allocates (a concise/object-literal
method's `this` clone); `CreateListFromArrayLike`'s array-like path reused a
raw source-object pointer read once before its per-index allocating loop
instead of re-deriving it after each allocation; and a `(...fixed, ...rest)`
body that also synthesizes `arguments` could hand its callee the rest
array's pre-move address once the `arguments` array's allocation moved it. A
new `rebind_explicit_this_allocates` predicate lets `Reflect.apply`'s common
(non-cloning) path stay allocation-free and unrooted; only the one shape that
actually allocates pays for rooting, so the fix measures ~0% instead of the
+38.8% an earlier, unconditionally-rooted attempt cost. New regression tests
arm a named collection point at each fixed allocation and assert the callee
observes post-collection addresses.
