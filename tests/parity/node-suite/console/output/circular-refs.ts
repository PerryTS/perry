// Issue #1204: `console.log` and friends should detect circular references
// and emit Node-compatible markers — `<ref *N>` at the cycle's head plus
// `[Circular *N]` at the back-edge — instead of recursing until the depth
// fallback collapses to `[Object]` / `[Array]`. Complements the
// `json-circular` fixture (which exercises a `%j`-formatted single
// self-ref); this one covers self-ref, two-object mutual cycle, and
// self-referencing array variants.

interface SelfRef {
  self?: SelfRef;
}
const a: SelfRef = {};
a.self = a;
console.log(a);

interface ABNode {
  name: string;
  ref?: ABNode;
}
const b: ABNode = { name: "b" };
const c: ABNode = { name: "c" };
b.ref = c;
c.ref = b;
console.log(b);

const arr: unknown[] = [];
arr.push(arr);
console.log(arr);
