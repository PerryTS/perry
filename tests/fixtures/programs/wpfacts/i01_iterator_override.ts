// The iteration protocol replaced by compiled code: one array's own
// `[Symbol.iterator]`, `Array.prototype[Symbol.iterator]`, and the shared
// array-iterator prototype's `next`. Each loop variable / `next().value`
// may then be a `B`, so no `.a` read may be proven over `A` alone.
// WPFACTS: site .a off
class A {
  a: number;
  constructor(a: number) {
    this.a = a;
  }
}
class B {
  b: string;
  a: string;
  constructor() {
    this.b = "x";
    this.a = "b-a";
  }
}

function joinA(arr: A[]): string {
  let s = "";
  for (const v of arr) s += v.a + ",";
  return s;
}

// 1. An array's own iterator method.
const xs: any = [new A(1), new A(2)];
xs[Symbol.iterator] = function* () {
  yield new B();
};
console.log(joinA(xs));

// 2. Array.prototype[Symbol.iterator].
const ys = [new A(3), new A(4)];
const origIter = (Array.prototype as any)[Symbol.iterator];
(Array.prototype as any)[Symbol.iterator] = function* (this: any[]) {
  for (let i = this.length - 1; i >= 0; i--) yield this[i];
  yield new B();
};
console.log(joinA(ys));
const [first, second] = ys as any;
console.log(first.a, second.a);
(Array.prototype as any)[Symbol.iterator] = origIter;

// 3. The array-iterator prototype's next, used by a hand-written loop.
const iterProto = Object.getPrototypeOf([][Symbol.iterator]());
const origNext = iterProto.next;
iterProto.next = function (this: any) {
  const r = origNext.call(this);
  return r.done ? r : { value: new B(), done: false };
};
const zs = [new A(5)];
const it = zs[Symbol.iterator]();
for (let r = it.next(); !r.done; r = it.next()) console.log(r.value.a);
iterProto.next = origNext;
console.log(joinA([new A(6)]));
