**"Which class's `.prototype` is this object?" is now O(1)** (#9180). Every
`Object.defineProperty`, `Object.getOwnPropertyDescriptor` and `delete` asked
`class_id_for_decl_prototype_object` about its receiver, and the answer was a
linear scan of every materialized declared-class prototype. The comment above
it explained that the table was small and the path was a cold reflection path;
a bundled application falsifies both — esbuild's `__export(exports, { … })`
runs `defineProperty` thousands of times during module init, the receivers are
never prototypes, and a miss walked the whole table. It was 3.10% of
`cc --help`.

The registry now carries its own inverse. `CLASS_DECL_PROTOTYPE_OBJECTS` holds
a `DeclPrototypeTable` whose two maps are private to one file, so the six
existing mutation sites — the store, both GC root scanners, the per-slot GC
step, the test reset and the test seeds — go through methods that update both
directions together, and a seventh cannot be written without editing that file.
That matters more than it sounds: an earlier pointer-keyed cache with
hand-placed invalidation went stale at the sites it missed, and the symptom was
not a crash but `getOwnPropertyDescriptor(C.prototype, "g")` quietly returning
`undefined` where node returns an accessor descriptor.

Two things keep it honest beyond privacy. The reverse map is an exact inverse
only while one address belongs to one class id and is never re-pointed;
`insert` is where either could break, checks both while inserting with one
extra reverse lookup, and on anything unusual abandons the index for good and
answers from the same authoritative forward-table scan as before. In a
`debug_assertions` build every reverse lookup is compared against that scan, so
the whole runtime test suite is checking the index rather than trusting it.

Measured on Linux with 400 declared-class prototypes materialized, per
operation on non-prototype receivers: `getOwnPropertyDescriptor` 420 → 200 ns
(2.10×), `defineProperty` 1585 → 1055 ns (1.50×), `delete` 2685 → 2445 ns. The
scan's signature was the slope — `getOwnPropertyDescriptor` cost 180/210/305/420
ns at 0/50/200/400 prototypes before, and is flat at ~200 ns after.
