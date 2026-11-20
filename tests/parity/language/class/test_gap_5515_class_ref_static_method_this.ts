// Issue #5515 (follow-up to #5437/#5513): when a static DATA property holding a
// callable is invoked as a method on a class-reference receiver (`C.m()`), the
// callee resolves and runs, but `this` must be bound to the class ref itself.
// A class reference reaches the call site as an INT32-tagged class id; sloppy
// `this` resolution (`js_implicit_this_get_sloppy`) and explicit-receiver
// coercion (`Function.prototype.call`) both boxed that int32 into a Number
// wrapper, so a regular-function static observed `this !== C` and lost access to
// the static chain. A class ref is conceptually the constructor OBJECT and must
// pass through unboxed.
//
// Expected output:
// fused: this===C
// call: this===C
// apply: this===C
// prop: V

function mk() {
  class C {}
  (C as any).tag = "C";
  (C as any).viaThis = function () {
    return (this as any) === C ? "this===C" : "this!==C";
  };
  return C;
}
const C = mk();
// Fused method call on a class-reference receiver.
console.log("fused:", (C as any).viaThis());
// Explicit `this` via Function.prototype.call / .apply.
console.log("call:", (C as any).viaThis.call(C));
console.log("apply:", (C as any).viaThis.apply(C));

// Reading a static data property through `this`.
function mk2() {
  class C {}
  (C as any).value = "V";
  (C as any).viaThis = function () {
    return (this as any).value;
  };
  return C;
}
console.log("prop:", (mk2() as any).viaThis());
