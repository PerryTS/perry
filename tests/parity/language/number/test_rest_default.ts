// `a[k] ?? <int32 literal>` over an Array stored at rest is an i32: the
// slot's word below the floor, else the literal for `undefined`
// (PERRY_REST_DEFAULT). Exact against node with the rule on, off and in
// check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

// 1. The shape: a board read through a helper with a default, in every
// consumer kind: a compare, an int local, arithmetic, a string.
class Board {
  cells: number[] = new Array<number>(64).fill(0);
  at(square: number): number {
    return this.cells[square] ?? 0;
  }
  other(square: number): number {
    return this.cells[square] ?? -7;
  }
  put(square: number, piece: number): void {
    this.cells[square] = piece;
  }
}
const b = new Board();
for (let i = 0; i < 64 * RUN; i++) b.put(i, (i % 13) - 6);
let empty = 0;
let sum = 0;
let prod = 0;
let text = "";
for (let i = -5; i < 70 * RUN; i++) {
  if (b.at(i) === 0) empty++;
  const p = b.at(i);
  sum = (sum + p * (i + 6)) | 0;
  prod += b.at(i) * 0.5;
  if (b.other(i) < -6) text += i + ":" + b.other(i) + " ";
}
console.log("board", empty, sum, prod, text);

// 2. A direct read with a default, keys below 0, in range and past the end.
const cells: number[] = new Array<number>(16).fill(0);
function put(i: number, v: number): void {
  cells[i] = v;
}
function get(i: number): number {
  return cells[i] ?? 9;
}
for (let i = 0; i < 16 * RUN; i++) put(i, i * i - 100);
const out2: number[] = [];
for (let i = -3; i < 20 * RUN; i++) out2.push(get(i));
console.log("direct", out2.join(","));

// 3. A length that moves inside the reserved capacity: slots past the
// length are holes, read as the default.
const grow: number[] = new Array<number>(8).fill(0);
function gput(i: number, v: number): void {
  grow[i & 15] = v & 1023;
}
function gget(i: number): number {
  return grow[i] ?? -1;
}
for (let i = 0; i < 8 * RUN; i++) gput(i, i * 3 + 1);
gput(12, 77);
const out3: number[] = [];
for (let i = 0; i < 18 * RUN; i++) out3.push(gget(i));
console.log("grow", grow.length, out3.join(","));

// 4. Defaults that are no int32 literal (a fraction, -0, a string, a
// variable): the read keeps its double.
const d: number[] = new Array<number>(8).fill(0);
function dput(i: number, v: number): void {
  d[i] = v;
}
for (let i = 0; i < 8 * RUN; i++) dput(i, i - 3);
function defaults(i: number, v: number): string {
  return [d[i] ?? 0.5, Object.is(d[i] ?? -0, -0), d[i] ?? ("x" as any), d[i] ?? v, d[i] ?? 2147483647, d[i] ?? -2147483648].join("/");
}
console.log("defaults", defaults(2, 4), defaults(8, 4), defaults(-1, 4), defaults(3, 4));

// 5. An Array that is not at rest (a fraction, a named property) keeps the
// generic read: its `??` sees the real value.
const loose: number[] = new Array<number>(8).fill(0);
loose[2] = 1.5;
(loose as any)[-1] = "minus";
function lget(i: number): any {
  return loose[i] ?? 0;
}
const out5: any[] = [];
for (let i = -2; i < 10 * RUN; i++) out5.push(lget(i));
console.log("loose", out5.join(","));
