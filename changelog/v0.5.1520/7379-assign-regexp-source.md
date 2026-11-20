**Fixed** `Object.assign({}, /re/)` and `{ ...\/re\/ }` type-confused the RegExp
source as a plain object, reading `ObjectHeader.keys_array` out of a
`RegExpHeader`'s fields and calling `js_array_length` on the resulting garbage
pointer.

`js_object_assign_one` skips exotic sources (Map/Set/Promise/Date/WeakMap) whose
header layout is not an `ObjectHeader`, and classifies them by GC type. A RegExp
is allocated as `gc_malloc(GC_TYPE_OBJECT)`, so it passes the `== GC_TYPE_OBJECT`
test that excludes the others and falls into the plain-object key walk. The slot
that lands at `ObjectHeader.keys_array`'s offset in a `RegExpHeader` is not a
keys array, so `js_array_length` reads a `GcHeader` at `garbage - 8`.

Unprotected this silently walks unrelated memory and usually still prints the
right answer; under `#7341`'s from-space quarantine the address is a retired
protected page and the process takes SIGBUS. This is 1 of the 8 catches that
remained open after #7373-#7376, and the only one whose root cause was a type
confusion rather than a rooting-order defect.

Per CopyDataProperties a RegExp exposes no own enumerable string keys through
this path, so skipping it matches Node: `Object.assign({}, /x/g)` is `{}`.
Verified with `test_gap_object_assign_collection`, which is now byte-identical
to Node **under quarantine**; 40 regex/assign/spread/object gap tests and 43
regex unit tests unchanged.
