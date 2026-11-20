"use strict";
// #11419: a callback the runtime invokes with no receiver — a `sort`
// comparator, a `reduce` / `reduceRight` callback — and a plain call of a
// function value must see `this === undefined`, even when the call happens
// inside a method whose receiver is still sitting in the implicit-`this` cell.
// Before the fix the comparator / reducer saw the enclosing method's receiver.

const seen: string[] = [];
function note(tag: string, self: any): void {
  if (self !== undefined) seen.push(tag + ":" + typeof self);
}

function cmp(this: any, a: number, b: number) { note("sort", this); return a - b; }
function cmpBig(this: any, a: bigint, b: bigint) { note("sortBig", this); return a < b ? -1 : a > b ? 1 : 0; }
function red(this: any, acc: number, v: number) { note("reduce", this); return acc + v; }
function zero(this: any) { return typeof this; }
function one(this: any, a: number) { return typeof this; }
function two(this: any, a: number, b: number) { return typeof this; }
function three(this: any, a: number, b: number, c: number) { return typeof this; }

const holder: any = {
  k: 1,
  arrays(this: any) {
    const a = [3, 1, 2];
    a.sort(cmp);
    const s = [3, 1, 2].toSorted(cmp);
    const r1 = [1, 2, 3].reduce(red, 0);
    const r2 = [1, 2, 3].reduce(red);
    const r3 = [1, 2, 3].reduceRight(red, 0);
    const r4 = [1, 2, 3].reduceRight(red);
    return a.join() + "|" + s.join() + "|" + r1 + "," + r2 + "," + r3 + "," + r4;
  },
  typed(this: any) {
    const f = new Float64Array([3, 1, 2]);
    f.sort(cmp);
    const t = new Int32Array([3, 1, 2]).toSorted(cmp);
    const b = new BigInt64Array([3n, 1n, 2n]);
    b.sort(cmpBig);
    const r1 = new Int32Array([1, 2, 3]).reduce(red, 0);
    const r2 = new Int32Array([1, 2, 3]).reduceRight(red);
    return f.join(",") + "|" + t.join(",") + "|" + b.join(",") + "|" + r1 + "," + r2;
  },
  arrayLike(this: any) {
    const o: any = { length: 3, 0: 3, 1: 1, 2: 2 };
    Array.prototype.sort.call(o, cmp);
    const r1 = Array.prototype.reduce.call({ length: 2, 0: 1, 1: 2 }, red, 0);
    const r2 = Array.prototype.reduceRight.call({ length: 2, 0: 1, 1: 2 }, red, 0);
    return o[0] + "," + o[1] + "," + o[2] + "|" + r1 + "," + r2;
  },
  plain(this: any) {
    const f0: any = zero;
    const f1: any = one;
    const f2: any = two;
    const f3: any = three;
    return [f0(), f1(1), f2(1, 2), f3(1, 2, 3), zero(), two(1, 2)].join(",");
  },
};

console.log("arrays", holder.arrays());
console.log("typed", holder.typed());
console.log("arrayLike", holder.arrayLike());
console.log("plain", holder.plain());

class Box {
  v = 7;
  run() {
    const out = [5, 4].sort(cmp).join() + "|" + [1, 2].reduce(red, 0);
    // The method's own `this` must survive the callbacks.
    return out + "|" + this.v;
  }
}
console.log("class", new Box().run());

console.log("leaks", seen.length === 0 ? "none" : seen.join(" "));
