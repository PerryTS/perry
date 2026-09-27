A function's own properties now live in the function object: an ordinary
shaped property object hangs off the closure header, traced by the collector
and installed with a write barrier. The three process-global closure side
tables (own properties, deleted keys, recorded prototypes), their young log
and their rekey/prune/scan passes are deleted. A function with only data
properties gets a keyed Function shape that is canonical per its property
object's key list.

The method-call site serves `F.m()` on such a function from that property
object (a new entry kind), so a namespace-style function's methods are called
directly like an ordinary object's.

Fixed, matching node: `bind` reads `length` through a getter; a deleted
`name` / `length` is inherited from `Function.prototype`; after
`Object.setPrototypeOf(fn, p)`, `fn.call` / `apply` / `bind` use `p`'s; and
calling a method a function object does not have (never set, deleted, or
absent from its prototype) throws a TypeError instead of returning `{}`.

Measured against the method-call base (instructions, real release profile,
5 interleaved rounds, each arm linking its own runtime): the Zod workload
1.609G -> 1.143G (-29.0%), the tsc workload 79.57G -> 78.03G (-1.9%).
