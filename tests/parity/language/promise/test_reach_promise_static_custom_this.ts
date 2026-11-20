// A Promise static called with another constructor as `this` builds its
// capability with that constructor (ECMA-262 27.2.1.5 NewPromiseCapability):
// `new C(executor)` runs C's body, and a C that never calls the executor
// leaves the capability without functions, a TypeError.
function Silent(this: any, _ex: any) {
  console.log("Silent constructed");
}
try {
  Promise.all.call(Silent, []);
  console.log("no throw");
} catch (e) {
  console.log("all:", e instanceof TypeError);
}

class Eager {
  constructor(ex: any) {
    console.log("Eager constructed");
    ex(
      (v: any) => console.log("resolved", v),
      () => {},
    );
  }
}
Promise.resolve.call(Eager, 7);
