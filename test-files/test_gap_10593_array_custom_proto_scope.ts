// #10593: `Object.setPrototypeOf(someArray, p)` used to flip a process-wide
// byte that every inline array-element guard loads, so ONE retargeted array
// moved every element access in the program off the fast path (and every
// element store onto the full write barrier — 33x whole-program on the
// issue's fixture). The fact is now carried on the retargeted array's own
// header. This pins the observable half: the retargeted array still reads,
// writes, pushes and pops through its custom chain, on every inline tier,
// while its neighbours keep ordinary semantics — including an array
// retargeted in the middle of a hot loop that already admitted it.

function show(v: unknown): string {
  if (v === undefined) return "undefined";
  if (v === null) return "null";
  if (typeof v === "object") return "object";
  return typeof v + "(" + String(v) + ")";
}

const log: string[] = [];

const proto: Record<string, unknown> = Object.create(Array.prototype);
Object.defineProperty(proto, "1", {
  configurable: true,
  get() {
    return "proto1";
  },
  set(v: unknown) {
    log.push("set1:" + show(v));
  },
});
proto["3"] = "proto3";
Object.defineProperty(proto, "4", {
  configurable: true,
  set(v: unknown) {
    log.push("set4:" + show(v));
  },
});

// Warm every inline tier on ordinary arrays first, so the guards are hot when
// the retarget lands.
function sumRange(arr: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    s += arr[i];
  }
  return s;
}
function readAt(arr: unknown[], i: number): unknown {
  return arr[i];
}
function writeAt(arr: unknown[], i: number, v: unknown): void {
  arr[i] = v;
}
const warm: number[] = [];
for (let i = 0; i < 64; i++) warm.push(i);
let warmSum = 0;
for (let k = 0; k < 200; k++) {
  warmSum += sumRange(warm, 64);
  writeAt(warm, k % 64, k % 64);
  warmSum += readAt(warm, k % 64) as number;
}
console.log("warm:", warmSum);

// 1. the retargeted array: holes and out-of-bounds reads walk the custom chain
const a: unknown[] = [10, , 30, , 50];
Object.setPrototypeOf(a, proto);
console.log("a proto is proto:", Object.getPrototypeOf(a) === proto);
console.log("a isArray:", Array.isArray(a));
console.log("a[0]:", show(a[0]), "a[1]:", show(a[1]), "a[3]:", show(a[3]));
console.log("a via readAt:", show(readAt(a, 1)), show(readAt(a, 3)), show(readAt(a, 9)));
const seen: string[] = [];
for (let i = 0; i < a.length; i++) seen.push(show(a[i]));
console.log("a loop:", seen.join(","));

// 2. a store into a hole hits the inherited setter and creates no own element
writeAt(a, 1, "w1");
a[1] = "w1b";
console.log("a has own 1:", Object.prototype.hasOwnProperty.call(a, "1"));
console.log("a[1] after writes:", show(a[1]));
// an own element is overwritten normally
a[0] = 11;
console.log("a[0] after write:", show(a[0]));

// 3. push at index 4 hits the inherited setter; length still advances
const c: unknown[] = [0, 0, 0, 0];
Object.setPrototypeOf(c, proto);
c.push("pushed");
console.log("c length after push:", c.length, "own 4:", Object.prototype.hasOwnProperty.call(c, "4"));

// 4. pop of a trailing hole reads through the chain
const d: unknown[] = [1, 2, 3, ,];
Object.setPrototypeOf(d, proto);
console.log("d pop:", show(d.pop()), "len:", d.length);

// 5. bystanders keep ordinary semantics, before and after other retargets
const e: unknown[] = [10, , 30, , 50];
console.log("e[1]:", show(e[1]), "e[3]:", show(e[3]), "e[9]:", show(readAt(e, 9)));
e[1] = "own1";
e.push("p");
console.log("e:", e.map(show).join(","), "own 1:", Object.prototype.hasOwnProperty.call(e, "1"));
const f: unknown[] = [1, 2, , 4];
console.log("f pop:", show(f.pop()), "f[2]:", show(f[2]));
console.log("warm again:", sumRange(warm, 64));

// 6. a retarget in the middle of a loop that already admitted the array
const g: unknown[] = new Array(6);
g[0] = "g0";
g[2] = "g2";
const midLoop: string[] = [];
for (let i = 0; i < g.length; i++) {
  if (i === 1) Object.setPrototypeOf(g, proto);
  midLoop.push(show(g[i]));
}
console.log("g mid-loop:", midLoop.join(","));

// 7. an array held in an object field (no stack-local writeback slot)
class Holder {
  vals: unknown[] = new Array(5);
  put(i: number, v: unknown): void {
    this.vals[i] = v;
  }
}
const h = new Holder();
for (let i = 0; i < 5; i++) h.put(i, i * 2);
const h2 = new Holder();
Object.setPrototypeOf(h2.vals, proto);
for (let i = 0; i < 5; i++) {
  if (i !== 4) h2.put(i, i * 3);
}
h2.put(4, "via-field");
console.log("h:", h.vals.map(show).join(","));
console.log("h2 own 4:", Object.prototype.hasOwnProperty.call(h2.vals, "4"), "h2[3]:", show(h2.vals[3]));

// 8. resetting to Array.prototype restores ordinary lookups
const r: unknown[] = [1, , 3];
Object.setPrototypeOf(r, proto);
console.log("r[1] custom:", show(r[1]));
Object.setPrototypeOf(r, Array.prototype);
console.log("r[1] reset:", show(r[1]));

console.log("setter log:", log.join(" "));
