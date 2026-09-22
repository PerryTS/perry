`instanceof` no longer segfaults on short inline strings, and `Object.create(proto).constructor`
returns the real constructor. The receiver is now resolved per value kind rather than assumed to be a
heap pointer, with the prototype and class-registry paths updated to match.

The string crash took down ajv, and with it every fastify schema route; the `constructor` defect
crashed lodash's `isEqual`. `new EventEmitter() instanceof EventEmitter` is fixed as a direct
consequence.

A related shape, `class Sub extends EventEmitter {}` followed by `new Sub() instanceof
EventEmitter`, took a separate fix: that call compiles through the dynamic-dispatch instanceof path
(the RHS resolves via a native-module lookup), which never registered or consulted the class-chain
parent edge that `extends Array`/`Map`/`Set`/`Error` subclassing already uses. A subclass instance is
a real object carrying its own class id, not a handle and not prototype-linked to
`EventEmitter.prototype`, so it was invisible to the handle/prototype probes on that path and always
answered `false`. EventEmitter's reserved class id is now a valid `extends` parent, and the
dynamic-dispatch branch delegates to the class-chain walk first, falling back to the prototype walk
for `util.inherits`-style shapes.

A CodeRabbit review pass on this PR also found that `instanceof`'s dynamic-RHS classification (and
`value_is_callable`) trusted the INT32-class-ref tag band alone, without checking the class id was
actually registered. A JS program can construct a `number` sharing that same tag band directly (via
`DataView`), which was then misread as a class reference instead of correctly reaching the
unresolved-RHS `TypeError`. Both sites now go through the same `class_ref_id` helper (which also
checks `is_class_id_registered`) that the rest of the crate already uses for this.
