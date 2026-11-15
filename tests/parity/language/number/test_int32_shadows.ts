// The ToInt32 of an immutable local that holds only Numbers and `undefined`
// is computed once (PERRY_INT32_SHADOWS) and read by every bitwise operand.
// Exact against node with the rule on, off and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

// 1. The shape: a packed move decoded field by field. The moves come from a
// for-of over an Array with index stores, so the facts give the parameter
// Number or `undefined`.
class Engine {
  board: number[] = new Array<number>(128).fill(0);
  h = 0;
  gen(): number[] {
    const moves: number[] = [];
    for (let from = 0; from < 120; from++) moves.push(from | (((from * 7) & 127) << 7) | ((from & 3) << 14) | ((from & 1) << 17));
    return moves;
  }
  order(moves: readonly number[]): number[] {
    const result = moves.slice();
    for (let i = 1; i < result.length; i++) {
      const move = result[i] ?? 0;
      let j = i - 1;
      while (j >= 0 && ((result[j] ?? 0) & 127) < (move & 127)) { result[j + 1] = result[j] ?? 0; j--; }
      result[j + 1] = move;
    }
    return result;
  }
  make(move: number): number {
    const from = move & 127;
    const to = (move >> 7) & 127;
    const promo = (move >> 14) & 7;
    if ((move & 131072) !== 0) this.h = (this.h + 1) | 0;
    this.board[from] = to + promo;
    this.h = (this.h * 31 + (move ^ (move << 3))) | 0;
    return from + to;
  }
}
const e = new Engine();
let s1 = 0;
for (let round = 0; round < 20 * RUN; round++) for (const move of e.order(e.gen())) s1 = (s1 + e.make(move)) | 0;
console.log("moves", s1, e.h, e.board.slice(0, 16).join(","));

// 2. `undefined` really arrives: a hole in the iterated Array.
class Holes {
  h = 0;
  use(v: number): number {
    return ((v & 255) ^ (v >> 8) ^ (v << 2)) | 0;
  }
}
const ho = new Holes();
const sparse: number[] = [1, 2, 3];
sparse[6] = 70000;
let s2 = "";
for (const v of sparse) s2 += ho.use(v) + ",";
console.log("holes", s2);

// 3. Numbers ToInt32 has to wrap or zero: fractions, -0, NaN, Infinity,
// values past 2^32 and past 2^63.
function wide(v: number): string {
  return [v & 65535, v >> 4, v << 1, v ^ 1, v | 0].join("/");
}
const out3: string[] = [];
for (const v of [1.9, -1.9, -0, NaN, Infinity, -Infinity, 4294967301, -4294967301, 2 ** 53, 1e20, -1e20, 2147483648]) out3.push(wide(v));
console.log("wide", out3.join(" "));

// 4. Not admitted: a local that is written, one that may be a string, a
// boolean, null, an object with valueOf (ToInt32 runs user code at each
// use), a BigInt.
function written(v: number): number {
  const a = v & 7;
  v = v + 1;
  return a + (v & 7) + (v >> 1);
}
let calls = 0;
const obj = { valueOf() { calls++; return 12; } };
function loose(v: any): string {
  return [v & 7, v >> 1, v ^ 3].join("/");
}
function bigs(v: any): string {
  try {
    return String((v & 7n) + (v >> 1n));
  } catch (err) {
    return (err as Error).name;
  }
}
let s4 = 0;
for (let i = 0; i < 9 * RUN; i++) s4 += written(i);
console.log(
  "loose",
  s4,
  loose("13"),
  loose(true),
  loose(null),
  loose(undefined),
  loose(obj),
  calls,
  loose([5]),
  bigs(13n),
  bigs(13),
);

// 5. A `const` in a loop body, re-initialized each iteration, and one a
// closure reads.
function consts(xs: number[]): number {
  let acc = 0;
  for (let i = 0; i < xs.length + 2; i++) {
    const v = xs[i];
    acc = (acc + ((v as number) & 15) + ((v as number) >> 4)) | 0;
    const f = () => ((v as number) & 3) + ((v as number) >> 2);
    acc = (acc + f()) | 0;
  }
  return acc;
}
console.log("consts", consts([17, 255, 4096, 65537]));
