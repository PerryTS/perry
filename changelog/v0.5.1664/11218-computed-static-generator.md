Fix `this` reads in generators declared with a computed static method key
(#11184). Their resume closures now receive the same receiver context as named
static generators, preventing class references from taking instance-only field
read paths.

Cover synchronous and asynchronous computed generators, private fields,
well-known symbol keys, and instance-method controls. Add a transform regression
that checks receiver captures, instance context, and async-generator registration
across named and computed methods.
