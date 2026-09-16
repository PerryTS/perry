Cut the instructions compiled TypeScript executes per operation, from an audit
that measured 93 probes three ways: instructions retired per call, emitted
instructions per symbol at `-Os`, and the same probes under the pinned Node.
Across the probe set the summed per-call cost falls 35,484 → 20,334 (−42.7%)
and the probe module's emitted code falls 4.8%.

Two correctness fixes came out of the measurements. A typed-array element store
wrote NaN-boxed values raw once a site's kind cache was warm, so `f64[i] = true`
read back `true` and a string, boolean or `null` stored `0` into an integer
kind; the inline arm and the runtime fast store now admit plain doubles only,
leave everything else to the setter's ToNumber, and use the exact modular
ToInt32 (the old truncation was poison for `|v| >= 2^63`). Array destructuring
called the iterator's `return()` when `next()`, or the result's `done`/`value`
getter, threw, where the spec marks the iterator done and skips IteratorClose.

The largest performance fix is cross-module class identity. A module mints the
ShapeId of every class it allocates — imported stubs included — in its
string-pool initializer, but only the defining module can mint the typed id
(#8405). Whenever a consumer's pool ran first (the entry module, an import
cycle, a deferred module's cycle) the same runtime class carried two
identities, and every instance the consumer allocated missed the defining
module's exact field-store guards: 2,664 instructions per store against 88.
Imported stubs now register their ShapeId/header-image global addresses and
`js_gc_typed_shape_id_for_keys` rewrites them in any init order.

The rest removes work that was provably unnecessary at the site that paid for
it: parameter guards and `typeof`/`switch`/`Math.max` literal tests that were
runtime calls become inline bit tests; a specialized clone whose IR matches its
generic sibling no longer emits `js_param_type_guard` at all; number parameters
test plain doubles before int32 boxes; declared typed arrays and unproven array
indexes take the inline element paths an `any` receiver already used; array
literals, pushes, rest bundles and `Map` stores skip layout notes and barrier
decode for values that cannot hold a pointer; `try` entry no longer captures
savepoints for subsystems no thread has used; closure births skip the newborn
barrier while no cycle runs; and numbers format into a stack buffer instead of
a temporary heap string.
