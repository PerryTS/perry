Fixed a capture-free class expression evaluated inside a function returning
the same constructor on every evaluation (#11298). `function makeClass() {
return class {}; }` lowered to one shared template `ClassRef`, so
`makeClass() === makeClass()` held and a property defined on one evaluation's
prototype leaked into instances of another. Such class expressions now
lower to a per-evaluation heap class object (`ClassExprFresh`), which carries
its own prototype (#11043). Their inferred local binding no longer takes the
static `new C()` alias, so an instance's prototype is this evaluation's
`C.prototype`. Module-top class expressions still evaluate once through the
shared template. Heritage class expressions with static methods, and class expressions
extending a native-module class (`extends AsyncResource`, #10623), keep the
shared template path, as before.

