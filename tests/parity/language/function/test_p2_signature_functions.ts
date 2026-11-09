// P2 typed signatures (DESIGN-p2 rev 2.3 §5): the function controls. Each
// line is exact against node with the fact on, off and in check mode;
// scripts/p2-signature-controls.py asserts the IR shape of each.

// S1: chess `encode`, with 2 and with 4 arguments (literal defaults).
function encode(from: number, to: number, promotion = 0, flags = 0): number {
  return from | (to << 7) | (promotion << 14) | flags;
}
let s1 = 0;
for (let i = 0; i < 64; i++) {
  s1 = (s1 + encode(i, 63 - i)) | 0;
  s1 = (s1 ^ encode(i, i + 1, 3, 1 << 20)) | 0;
}
console.log("S1", s1);

// S2: one caller passes 1.5, so that parameter is F64.
function twice(x: number): number {
  return x + x;
}
let s2 = 0;
for (let i = 0; i < 8; i++) s2 += twice(i);
console.log("S2", s2, twice(1.5));

// S3: defaults: passed undefined, a computed maybe-undefined argument, an
// indirect call with an omitted argument, a timer, and E1's TDZ.
function scale(x: number, k = 2): number {
  return x * k;
}
console.log("S3", scale(3), scale(11, undefined));
function add(a: number, b = 1): number {
  return a + b;
}
function maybe(i: number): number | undefined {
  return i % 2 === 0 ? undefined : i;
}
let s3 = 0;
for (let i = 0; i < 4; i++) s3 += add(i, maybe(i));
const fns = [add];
console.log("S3", s3, fns[0](9));
function report(x: number, k = 3): void {
  console.log("S3 timer", x * k);
}
report(1);
setTimeout(report, 0, 5);
function tdz(a = b, b = 1): number {
  return a;
}
try {
  console.log("S3", tdz());
} catch (e) {
  console.log("S3", e instanceof ReferenceError);
}

// S4: called directly and through `map`.
function sq(x: number): number {
  return x * x;
}
let s4 = 0;
for (let i = 0; i < 5; i++) s4 += sq(i);
console.log("S4", s4, [1, 2, 3].map(sq).join(","));

// S7: -0 passed.
function inv(x: number): number {
  return 1 / x;
}
console.log("S7", inv(2), inv(-0), inv(4));

// S8: recursion and mutual recursion.
function fib(n: number): number {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}
function isEven(n: number): boolean {
  return n === 0 ? true : isOddM(n - 1);
}
function isOddM(n: number): boolean {
  return n === 0 ? false : isEven(n - 1);
}
console.log("S8", fib(20), isEven(10), isOddM(7));

// S9: a throw through a typed call under try/catch/finally.
function boom(n: number): number {
  if (n > 3) throw new Error("big " + n);
  return n * 2;
}
let s9 = 0;
try {
  for (let i = 0; i < 10; i++) s9 += boom(i);
} catch (e) {
  console.log("S9", (e as Error).message);
} finally {
  console.log("S9 acc", s9);
}

// S12: call, apply and bind beside a direct call.
function plus1(n: number): number {
  return n + 1;
}
const o12 = { tag: 1 };
console.log("S12", plus1(5), plus1.call(o12, 1), plus1.apply(o12, [1]), plus1.bind(o12)(1));

// S13: allocation between typed calls (moving GC).
function mix(a: number, b: number): number {
  return (a * 31 + b) | 0;
}
let s13 = 0;
const keep: number[][] = [];
for (let i = 0; i < 20000; i++) {
  keep.push([i, i + 1]);
  if (keep.length > 100) keep.shift();
  s13 = mix(s13, keep[keep.length - 1][0]);
}
console.log("S13", s13, keep.length);

// S15: a module-global counter fed back through a typed parameter.
let counter = 0;
function bump(n: number): number {
  return n + 1;
}
for (let i = 0; i < 5; i++) counter = bump(counter);
console.log("S15", counter, bump.call(null, counter));

// S16: a two-body call site through an array.
const fs16 = [(x: number) => x, (x: number) => x * 2];
console.log("S16", fs16[0](3) + 0.5, fs16[1](3) + 0.5);

// S17: `new` of a typed plain function.
function dbl(x: number): number {
  return x * 2;
}
const made = new (dbl as any)(3);
console.log("S17", typeof made === "object", dbl(1) + 1);

// S18: deep recursion is a catchable RangeError.
function deep(n: number): number {
  return n === 0 ? 0 : 1 + deep(n - 1);
}
try {
  console.log("S18", deep(1e7) > 0 && false);
} catch (e) {
  console.log("S18", e instanceof RangeError);
}

// S21: an integer caller beside one passing 0.5 (signature F64).
function half(x: number): number {
  return x / 2;
}
let s21 = 0;
for (let i = 0; i < 10; i++) s21 += half(i);
s21 += half(0.5);
console.log("S21", s21);

// S22: a typed_abi-eligible helper also called with "7" through apply.
function tri(n: number): number {
  return (n * (n + 1)) / 2;
}
let s22 = 0;
for (let i = 0; i < 10; i++) s22 += tri(i);
console.log("S22", s22, tri.apply(null, ["7" as any]));

// S23: a negative index beside in-range stores.
const board: number[] = new Array(128).fill(0);
function setSq(sq: number, v: number): void {
  board[sq] = v;
}
for (let i = 0; i < 128; i++) setSq(i, i & 7);
setSq(-16, 1);
console.log("S23", board.length, board[5], (board as any)[-16], Object.keys(board).length);

// T4a: int32 overflow of an I32-parameter function.
function addI(a: number, b: number): number {
  return a + b;
}
function addW(a: number, b: number): number {
  return (a + b) | 0;
}
let t4a = 0;
for (let i = 0; i < 4; i++) t4a = addW(t4a, addI(i, i));
console.log("T4a", t4a, addI(2147483647, 1), addW(2147483647, 1));

// T4b: index edges: out of range, a fraction and a hole; and an in-range-only
// variant.
const holey: number[] = new Array(128);
for (let i = 0; i < 128; i++) if (i !== 7) holey[i] = i * 2;
function at(i: number): number | undefined {
  return holey[i];
}
let t4b = 0;
for (let i = 0; i < 128; i++) t4b += at(i) ?? 0;
console.log("T4b", t4b, at(200), at(1.5), at(7));
const dense: number[] = [];
for (let i = 0; i < 128; i++) dense.push(i + 1);
function atIn(i: number): number {
  return dense[i];
}
let t4b2 = 0;
for (let i = 0; i < 128; i++) t4b2 += atIn(i);
console.log("T4b", t4b2);

// T4d: a typed_i1 function stays untyped.
function isOdd(n: number): boolean {
  return (n & 1) === 1;
}
let t4d = 0;
for (let i = 0; i < 10; i++) if (isOdd(i)) t4d++;
console.log("T4d", t4d);
