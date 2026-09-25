Fixed a capture-free class expression evaluated inside a function returning
the same constructor on every evaluation (#11298). `function makeClass() {
return class {}; }` lowered to one shared template `ClassRef`, so
`makeClass() === makeClass()` held and a property defined on one evaluation's
prototype leaked into instances of another. Such class expressions now
lower to a per-evaluation heap class object (`ClassExprFresh`), which carries
its own prototype (#11043). Their inferred local binding no longer takes the
static `new C()` alias, so an instance's prototype is this evaluation's
`C.prototype`. Module-top class expressions still evaluate once through the
shared template. Heritage class expressions with static methods keep the
shared template path, as before.

A per-evaluation prototype now also links to a static native parent. `return
class extends EventEmitter {}` pins its parent as a ClassRef to EventEmitter's
reserved builtin id, which `class_ref_id` does not resolve, so the evaluation
prototype skipped `EventEmitter.prototype`. It now resolves through
`reserved_native_parent_prototype_bits`, the same path the declared-class
prototype uses (#10599).
