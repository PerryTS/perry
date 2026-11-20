// Issue #685: a class expression returned from a factory function with
// a static field initializer that references the factory's parameter.
// Pre-fix, Perry either lifted this initializer out of the factory or
// specialized the generic rest parameter as a scalar. The static field
// must be initialized when the class expression is evaluated and see the
// per-call rest array.

function makeWrapper<const Params extends ReadonlyArray<string>>(...params: Params) {
  return class WrapperClass {
    static params = params.slice()
  }
}

console.log("[1] before makeWrapper");
const W = makeWrapper("a", "b", "c");
console.log("[2] after makeWrapper");
// The static field must be a fresh array copy of the rest parameter, not
// a scalar argument.
console.log("[3] params type:", typeof W.params);
console.log("[4] params isArray:", Array.isArray(W.params));
console.log("[5] params json:", JSON.stringify(W.params));
