// #11394: replacing a builtin PROTOTYPE method must reach every call of it —
// proven receivers (which fold to an intrinsic), `any` receivers (which the
// runtime dispatcher answers by name) and function receivers alike.

const log: string[] = [];
const origPush = Array.prototype.push;
(Array.prototype as any).push = function (this: any[], ...xs: any[]) {
  origPush.call(log, "push:" + xs.join(","));
  return origPush.apply(this, xs);
};
const arr: number[] = [1, 2];
arr.push(3); // proven array
const anyArr: any = [9];
anyArr.push(8); // any receiver
function viaParam(a: any) {
  a.push(7);
}
viaParam(anyArr);
const more = [4, 5];
arr.push(...more); // spread arguments
const holder = { list: [] as number[] };
holder.list.push(6); // member receiver
class Bag {
  items: number[] = [];
  add(x: number) {
    this.items.push(x); // `this.field` receiver
    return this.items.length;
  }
}
new Bag().add(10);
(Array.prototype as any).push = origPush;
arr.push(11); // restored: the builtin again, and not logged

const origGet = Map.prototype.get;
(Map.prototype as any).get = function (this: Map<any, any>, k: any) {
  origPush.call(log, "get:" + k);
  return origGet.call(this, k);
};
const m = new Map<string, number>([["a", 1]]);
m.get("a");
(m as any).get("a");
(Map.prototype as any).get = origGet;

const origAdd = Set.prototype.add;
Object.defineProperty(Set.prototype, "add", {
  value: function (this: Set<any>, v: any) {
    origPush.call(log, "add:" + v);
    return origAdd.call(this, v);
  },
  writable: true,
  configurable: true,
});
const s = new Set<number>();
s.add(1).add(2);
Object.defineProperty(Set.prototype, "add", { value: origAdd, writable: true, configurable: true });

const origBind = Function.prototype.bind;
(Function.prototype as any).bind = function (this: any, t: any) {
  origPush.call(log, "bind");
  return origBind.call(this, t);
};
function g() {
  return 1;
}
g.bind(null);
(Function.prototype as any).bind = origBind;

// A write through an alias of the prototype counts too.
const AP: any = Array.prototype;
const origMap = AP.map;
AP.map = function (this: any[], fn: any) {
  origPush.call(log, "map");
  return origMap.call(this, fn);
};
const doubled = [1, 2, 3].map((x) => x * 2);
AP.map = origMap;

// An own property still beats the patched prototype, and a class's own method
// of the same name is not the builtin's.
const own: any = [0];
own.push = (x: any) => {
  origPush.call(log, "own:" + x);
  return -1;
};
(Array.prototype as any).push = function () {
  origPush.call(log, "proto");
  return -2;
};
own.push(1);
class Stack {
  push(x: number) {
    origPush.call(log, "Stack:" + x);
    return x;
  }
}
const st: any = new Stack();
st.push(2);
// Typed arrays inherit from %TypedArray%.prototype, not Array.prototype.
const typed = new Uint8Array([1, 2]).map((x) => x + 1);
(Array.prototype as any).push = origPush;

console.log(JSON.stringify(log));
console.log(arr.join(","), anyArr.join(","), holder.list.join(","), m.get("a"), s.size);
console.log(doubled.join(","), typed.join(","));
