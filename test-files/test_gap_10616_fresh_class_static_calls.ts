// #10616: static methods of a function-nested class whose constructor and
// static methods write a captured variable; a method site that serves a
// class object's own static methods must call each evaluation's own one.

function make(start: number) {
  var count = start;
  class K {
    constructor() {
      count++;
    }
    static bump() {
      count += 2;
      return this === K;
    }
    static read() {
      return count;
    }
  }
  return { K, get: () => count };
}

const one = make(0);
const two = make(100);
let same = 0;
for (let i = 0; i < 50; i++) {
  new one.K();
  if (one.K.bump()) same++;
}
console.log("one", one.get(), same, one.K.read());

// Alternating class objects at one call site.
const ks: any[] = [one.K, two.K];
for (let i = 0; i < 40; i++) ks[i & 1].bump();
console.log("alternating", one.get(), two.get());

// A static replaced or deleted on one class object.
const K1: any = one.K;
const orig = K1.bump;
K1.bump = function () {
  return "replaced";
};
console.log("replaced", K1.bump(), two.K.bump(), two.get());
delete K1.bump;
console.log("deleted", typeof K1.bump, K1.read());
K1.bump = orig;
console.log("restored", K1.bump(), one.get());
