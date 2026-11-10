// Fact IM, program-wide controls: an Array whose iteration is not the
// builtin walk is never immutable-walked. An own `@@iterator`, a replaced prototype
// and an Array subclass with its own iterator each make the Array something
// the facts do not call a plain program Array, so its for-of runs the
// iterator the program installed; the literals beside them keep the walk.
//
// Replacing `Array.prototype[Symbol.iterator]` or
// `%ArrayIteratorPrototype%.next` is refused by the compiled program's
// intrinsic lock (a TypeError, with or without this fact), so that control
// cannot be node-exact and is not in a parity fixture.
const out: string[] = [];

const A = [1, 2, 3, 4];
const B = [10, 20];

function walk(xs: number[]): string {
  let s = "";
  for (const v of xs) s += v + ",";
  return s;
}
let direct = "";
for (const v of A) direct += v + ",";
out.push("plain " + direct + walk(A) + walk(B));

// An own iterator on one Array.
const C = [5, 6, 7];
(C as any)[Symbol.iterator] = function* () {
  yield 99;
  yield 98;
};
direct = "";
for (const v of C) direct += v + ",";
out.push("own " + direct + walk(C));

// A prototype with another iterator.
const D = [1, 2, 3];
Object.setPrototypeOf(D, {
  *[Symbol.iterator]() {
    yield 42;
  },
});
direct = "";
for (const v of D as any) direct += v + ",";
out.push("proto " + direct);

// An Array subclass that iterates backwards.
class Backwards extends Array<number> {
  *[Symbol.iterator](): any {
    for (let i = this.length - 1; i >= 0; i--) yield this[i];
  }
}
const E = new Backwards();
E.push(1, 2, 3);
direct = "";
for (const v of E) direct += v + ",";
out.push("subclass " + direct);

// An array-like object with an iterator, walked by the same function as
// the literals.
const like = {
  length: 2,
  0: "x",
  1: "y",
  *[Symbol.iterator]() {
    yield "p";
    yield "q";
  },
};
direct = "";
for (const v of like) direct += v + ",";
out.push("like " + direct + walk(like as any));

// An index defined with a descriptor, and `Reflect.set`: either makes
// element descriptors reachable in the program, so no walk is decided by
// the facts and every for-of asks the runtime.
const O2 = [1, 2, 3];
Object.defineProperty(O2, 1, { value: 21, writable: true, enumerable: true, configurable: true });
const O3 = [1, 2, 3];
Reflect.set(O3, 1, 22);
const G = [1, 2, 3];
Object.defineProperty(G, 0, {
  get() {
    return 77;
  },
  enumerable: true,
  configurable: true,
});
direct = "";
for (const v of O2) direct += v + ",";
for (const v of O3) direct += v + ",";
for (const v of G) direct += v + ",";
out.push("defined " + direct + walk(O2) + walk(G));

// The literals after all of that: still their own elements.
direct = "";
for (const v of A) direct += v + ",";
out.push("after " + direct + walk(A) + walk(B));

console.log(out.join("\n"));
