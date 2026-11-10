// #11876: a hole read inside a counter loop over a typed array parameter must
// continue on the prototype chain (OrdinaryGet), whatever loop tier serves the
// element read. Before the fix the bounded-index tier read every hole as
// `undefined` even after `Array.prototype[2]` was defined; the straight-line
// read was already correct.
//
// Order matters: the first block runs before ANY prototype carries an index
// (holes read `undefined`), the second gives one array a custom prototype, and
// only the last gives a SHARED user array prototype an index (DEC 250:
// `Array.prototype` refuses program index adds).

class Body {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

function at(bs: Body[], i: number): Body {
  return bs[i];
}
function sumX(bs: Body[]): number {
  let s = 0;
  for (let i = 0; i < bs.length; i++) s += bs[i].x;
  return s;
}
function sumOrMinus(bs: Body[]): number {
  let s = 0;
  for (let i = 0; i < bs.length; i++) {
    const b = bs[i];
    s += b === undefined ? -1 : b.x;
  }
  return s;
}
function countUndef(bs: Body[]): number {
  let c = 0;
  for (let i = 0; i < bs.length; i++) if (bs[i] === undefined) c++;
  return c;
}
function sumNums(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    const v = xs[i];
    s += v === undefined ? -1 : v;
  }
  return s;
}
function joinStrs(xs: string[]): string {
  let out = "";
  for (let i = 0; i < xs.length; i++) out += String(xs[i]) + ";";
  return out;
}

function makeBodies(): Body[] {
  const bs = [new Body(1), new Body(2), new Body(3), new Body(4)];
  delete bs[2];
  return bs;
}
function makeSparse(): Body[] {
  const bs: Body[] = new Array(4);
  bs[0] = new Body(1);
  bs[1] = new Body(2);
  bs[3] = new Body(4);
  return bs;
}

// 1. no indexed property anywhere on the chain: a hole reads `undefined`
console.log("clean", sumOrMinus(makeBodies()), countUndef(makeBodies()), sumOrMinus(makeSparse()));
console.log("clean strs", joinStrs(["a", "b", , "d"] as string[]));

// 2. one array with a custom prototype that carries index 2
const custom = makeBodies();
const proto: any = Object.create(Array.prototype);
proto[2] = new Body(50);
Object.setPrototypeOf(custom, proto);
console.log("custom proto", at(custom, 2).x, sumX(custom), sumOrMinus(custom), countUndef(custom));
console.log("custom proto, plain array still clean", sumOrMinus(makeBodies()));

// 3. a shared user array prototype carries index 2
const shared: any = Object.create(Array.prototype);
shared[2] = new Body(100);
function adopt<E>(a: E[]): E[] {
  Object.setPrototypeOf(a, shared);
  return a;
}
const bs = adopt(makeBodies());
console.log("shared proto", at(bs, 2).x, sumX(bs), countUndef(bs), sumOrMinus(bs));
console.log("shared proto sparse", sumX(adopt(makeSparse())), sumOrMinus(adopt(makeSparse())));
const ns = adopt([1, 2, 3, 4]);
delete ns[2];
console.log("shared proto numbers", sumNums(ns));
const ss = adopt(["a", "b", "c", "d"]);
delete ss[2];
console.log("shared proto strings", joinStrs(ss));
// 4. an accessor on the shared prototype runs once per hole read
let getterCalls = 0;
delete shared[2];
Object.defineProperty(shared, 2, {
  configurable: true,
  get() {
    getterCalls++;
    return new Body(1000);
  },
});
console.log("getter", sumX(adopt(makeBodies())), getterCalls);
delete shared[2];
console.log("after delete", sumOrMinus(adopt(makeBodies())), countUndef(adopt(makeBodies())));
