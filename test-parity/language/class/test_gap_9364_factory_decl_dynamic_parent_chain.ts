// Issue #9364: each evaluation of a class declaration inside a factory owns
// its dynamic parent. Chaining the factory through its previous result must
// construct normally instead of recursing through the shared template id.
function mk(P: any): any {
  class D extends (P ?? Object) {}
  return D;
}

const A = mk(null);
const B = mk(A);
console.log("ok " + typeof new B());
