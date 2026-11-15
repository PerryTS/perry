// P3 B4b: a JS Number has one encoding, the plain double. The runtime no
// longer decodes an INT32-tagged word anywhere a value is consumed, so every
// integer below must reach the generic runtime paths as a double and print
// what node prints. Negative controls: integers held at rest as I32 words
// (an Array born by a fused fill, an integer class field) that also reach a
// generic reader, which must see Numbers, never slot words.
const dyn = (v: any): any => v;
const ints = [0, 1, -1, 7, 255, 65536, 2147483647, -2147483648, 4294967295];
const odd = [-0, 0.5, -1.5, NaN, Infinity, -Infinity, 2 ** 53];

// Equality, relational compare, loose equality through `any`.
let eq = 0;
for (const a of ints) {
  for (const b of ints) {
    const x = dyn(a);
    const y = dyn(b);
    if (x === y) eq += 1;
    if (x == String(b)) eq += 10;
    if (x < y) eq += 100;
    if (x >= y) eq += 1000;
  }
}
console.log("eq", eq);
console.log("zero", dyn(0) === dyn(-0), Object.is(dyn(0), dyn(-0)), dyn(NaN) === dyn(NaN));

// ToString, template, concat, join, JSON.
console.log(ints.map((n) => String(dyn(n))).join("|"));
console.log(ints.map((n) => `${dyn(n)}`).join("|"));
console.log("" + dyn(7) + dyn(-1) + dyn(2147483647));
console.log(JSON.stringify({ a: dyn(1), b: dyn(-2147483648), c: [dyn(0), dyn(-0), dyn(4294967295)] }));
console.log(JSON.stringify(odd), odd.map((n) => String(dyn(n))).join("|"));
console.log(JSON.stringify(ints, null, 0), JSON.stringify({ k: 3 }, null, dyn(0)));
console.log(dyn(255).toString(16), dyn(-255).toString(2), dyn(65536).toFixed(2), dyn(7).toLocaleString("en-US"));

// typeof, truthiness, arithmetic, unary, bitwise through `any`.
console.log(typeof dyn(5), !!dyn(0), !!dyn(5), !dyn(-1), dyn(5) ? "t" : "f");
console.log(dyn(5) + dyn(3), dyn(5) - dyn(8), dyn(5) * dyn(-3), dyn(7) / dyn(2), dyn(7) % dyn(-3), -dyn(0));
console.log(dyn(5) | 0, dyn(-1) >>> 0, dyn(2147483647) + 1, dyn(-2147483648) - 1, dyn(7) ** dyn(2));
console.log(Number(dyn(12)), BigInt(dyn(12)) + 1n, Math.abs(dyn(-12)), Math.round(dyn(2.5)), parseInt(String(dyn(42))));

// Keys: object, Array, Map, Set, typed array, Buffer, string index.
const o: any = {};
for (const n of ints) o[dyn(n)] = n;
console.log(Object.keys(o).join(","), o[dyn(7)], dyn(7) in o, 9 in o);
const arr: any[] = [10, 20, 30];
console.log(arr[dyn(1)], arr[dyn(-1)], arr.indexOf(dyn(30)), arr.includes(dyn(20)), arr.at(dyn(-1)));
arr[dyn(5)] = dyn(60);
console.log(arr.length, JSON.stringify(arr), arr.join("-"));
const m = new Map<any, any>();
const s = new Set<any>();
for (const n of ints) {
  m.set(dyn(n), n * 2);
  s.add(dyn(n));
}
m.set(dyn(-0), "z");
console.log(m.size, m.get(0), m.get(dyn(7)), m.has(dyn(4294967295)), s.size, s.has(dyn(-1)), s.has(dyn(8)));
const ta = new Uint8Array(dyn(4));
ta[dyn(1)] = dyn(300);
ta[dyn(2)] = dyn(-1);
console.log(ta.length, ta[dyn(1)], ta[dyn(2)], ta[dyn(9)], dyn(1) in ta, Array.from(ta).join(","));
const buf = Buffer.alloc(dyn(4));
buf[dyn(0)] = dyn(65);
buf.writeInt32LE(dyn(-2), 0);
console.log(buf.readInt32LE(dyn(0)), buf.indexOf(dyn(255)), buf.length, buf.toString("hex"));
console.log("abcdef"[dyn(2)], "abcdef".charCodeAt(dyn(1)), "abcdef".slice(dyn(1), dyn(-1)), "ab".repeat(dyn(2)), String.fromCharCode(dyn(65)));

// switch, sort, spread, destructuring, closures called with numbers.
function kind(v: any): string {
  switch (v) {
    case 0:
      return "zero";
    case 7:
      return "seven";
    case -1:
      return "minus";
    default:
      return "other";
  }
}
console.log(ints.map(kind).join(","));
console.log([...ints].sort((a, b) => dyn(a) - dyn(b)).join(","), Math.max(...ints), Math.min(...ints));
const call = (f: any, ...xs: any[]) => f(...xs);
console.log(call((a: any, b: any) => a * 100 + b, dyn(3), dyn(4)), call.length, [1, 2, 3].map(dyn).reduce((a, b) => a + b, 0));

// Negative control 1: an Array born by a fused fill and written with int32
// values in a loop (an at-rest candidate), read by proven code and then by
// generic readers.
function board(): number[] {
  const b = new Array(16).fill(0);
  for (let i = 0; i < 16; i++) b[i] = (i * 7) % 5 - 2;
  return b;
}
const cells = board();
let sum = 0;
for (let i = 0; i < 16; i++) sum += cells[i];
console.log(sum, cells.join(","), JSON.stringify(cells), cells.indexOf(2), [...cells].length, String(cells[3]));
console.log(Object.values(cells).length, cells.slice(2, 5), cells.map((v) => v * 2).join(""), cells.includes(-2));

// Negative control 2: integer class fields (I32 lane candidates) read by
// proven code and by generic readers.
class Counter {
  n: number;
  step: number;
  constructor(n: number, step: number) {
    this.n = n;
    this.step = step;
  }
  tick(): number {
    this.n = (this.n + this.step) | 0;
    return this.n;
  }
}
const c = new Counter(3, 4);
let t = 0;
for (let i = 0; i < 5; i++) t += c.tick();
console.log(t, c.n, JSON.stringify(c), Object.keys(c).join(","), Object.values(c).join(","), `${c.n}`, c.n === dyn(23));
console.log(c, { ...c }, Object.entries(c));

// A proven integer-only class whose instances no generic reader reaches
// stays at rest; its values still come out as Numbers.
class Sq {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  add(k: number): void {
    this.v = (this.v + k) | 0;
  }
}
const q = new Sq(1);
for (let i = 0; i < 10; i++) q.add(i);
console.log(q.v, q.v + 0.5, String(q.v), dyn(q.v) === 46, typeof q.v);
