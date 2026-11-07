A `#private` field now carries no value out of its class in the whole-program and per-unit facts,
and private fields get definite birth slots after the declared keys, derived from the same
layout codegen allocates. Static private fields are no longer published as class statics.
