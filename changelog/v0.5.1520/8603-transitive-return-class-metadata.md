Retain declared method, getter, and static-factory return types in imported class
metadata. Consumers now pull in dispatch metadata for classes reachable through
those results, including type-only aliases and non-exported classes in the
declaring module, without adding runtime import or initialization edges. This
lets chained calls such as `world.query(...).forEach(...)` use guarded direct
class dispatch instead of losing the returned receiver type at the module
boundary.
Imported static calls also retain their declared arity, rest shape, and hidden
`arguments` slot so cross-module default, rest, and `arguments` semantics match
the producer's ABI.
