### Changed

- `class X extends Array` instances now keep their indexed elements and `length` in a real elements store (`ObjectMeta.elements`) instead of shape-carried properties, so `push`/`pop`/`obj[i]` are element operations rather than property-shape transitions: −11.4% (add/remove) and −11.9% (entity cycle) on the wolf-ecs benchmarks. Semantics move toward node — `JSON.stringify` produces the array form, `Object.keys` no longer leaks `length`, and the mutator surface matches node exactly. `PERRY_ARRAY_SUBCLASS_ELEMENTS=0` restores the previous representation for bisecting.
