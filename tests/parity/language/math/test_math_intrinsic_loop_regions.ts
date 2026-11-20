// c1: Math.abs/floor/ceil/sqrt/min/max over proven Numbers and `c ? a : b`
// over inert operands are inert in loop regions. Values are node-exact
// (NaN, signed zero, Infinity), and the controls whose operands are not
// proven Numbers still run their user code in order.

function minmax(xs: number[], n: number): string {
  const out: string[] = [];
  for (let i = 0; i + 1 < n; i++) {
    const a = xs[i] as number;
    const b = xs[i + 1] as number;
    const hi = Math.max(a, b);
    const lo = Math.min(a, b);
    out.push(
      (Object.is(hi, -0) ? "-0" : String(hi)) + "/" + (Object.is(lo, -0) ? "-0" : String(lo)),
    );
  }
  return out.join(" ");
}
const vals = [NaN, 1, -0, 0, -0, Infinity, -Infinity, 2.5, -2.5, NaN];
console.log(minmax(vals, vals.length));

function unary(xs: number[], n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) {
    const x = xs[i] as number;
    const r = [Math.abs(x), Math.floor(x), Math.ceil(x), Math.sqrt(x)];
    s += r.map((v) => (Object.is(v, -0) ? "-0" : String(v))).join(",") + ";";
  }
  return s;
}
console.log(unary([-0, 0, -1.5, 1.5, -Infinity, Infinity, NaN, 4, -4, 0.25], 10));

// A counted loop whose body is only Math over Numbers and a conditional.
function passed(board: number[], file: number, rank: number, side: number): number {
  let hits = 0;
  for (let r = rank + side; r >= 0 && r < 8; r += side > 0 ? 1 : -1) {
    for (let f = Math.max(0, file - 1); f <= Math.min(7, file + 1); f++) {
      if (board[r * 8 + f] === (side > 0 ? -1 : 1)) hits++;
    }
  }
  return hits;
}
const board: number[] = [];
for (let i = 0; i < 64; i++) board.push(i % 7 === 0 ? -1 : i % 5 === 0 ? 1 : 0);
let total = 0;
for (let sq = 0; sq < 64; sq++) total += passed(board, sq & 7, sq >> 3, sq % 2 === 0 ? 1 : -1);
console.log(total);

// Empty min/max fold to their identities.
function empties(n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) s += String(Math.max()) + String(Math.min());
  return s;
}
console.log(empties(2));

// Control: an operand with a valueOf is not a proven Number; the call runs
// it, in order, once per iteration, and it may allocate.
function coerced(n: number): string {
  const log: string[] = [];
  const o = { valueOf() { log.push("v" + log.length); return -3; } };
  let s = 0;
  for (let i = 0; i < n; i++) s += Math.abs(o as unknown as number) + Math.max(i, o as unknown as number);
  return s + " " + log.join(",");
}
console.log(coerced(3));

// Control: a conditional with an object arm under `+` concatenates and runs
// toString.
function concat(n: number): string {
  const tag = { toString() { return "T"; } };
  let s: any = 0;
  for (let i = 0; i < n; i++) s = s + (i % 2 === 0 ? tag : 1);
  return String(s);
}
console.log(concat(4));
