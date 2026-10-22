Sync generator births no longer intern a shape per prototype, allocate a meta
record, or create the generator function's prototype object. Every sync
generator is born on one agent-lifetime layout: the `[[GeneratorState]]`
private slot, the generator brand, and a record-held [[Prototype]]
(`PROTO_ID_RECORD_HELD`). The resume record's sixth control holds the
prototype object, or the owner function while its original prototype is
unmaterialized; prototype walks read the record, and getPrototypeOf,
Reflect.getPrototypeOf, `__proto__` and setPrototypeOf materialize the
original first. A generator function declares `prototype` (writable,
non-enumerable, non-configurable) in its bag's birth, which also keeps the
original in a private slot, so a later `g.prototype = x` cannot change the
prototype of an instance born earlier. A function or non-heap object used as
`g.prototype` keeps a shape-held prototype.

The layout probe's complete walk now continues at the hop the fast chain
reached instead of re-deriving the previous link.

test_gap_generator_prototype_observables compares every order of
observation, reassignment and mutation with node.
