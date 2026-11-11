// Integer products, sums and differences under a test (CENSUS-residual-2
// lever 10): `piece * color > 0` over int32 values is one integer compare
// in the width the intervals prove (`expr::int_product`). The first half is
// tests the proof covers; the second half is the negative controls, where
// one part of the proof is missing (a value past 2^53, a rounding operand,
// a `-0` that is observed, NaN, Infinity, a fraction, `undefined`, a
// string, a leaf that writes) and the result must still be what doubles
// give.
const out: string[] = [];
// A bound the compiler cannot fold keeps the loops as written.
const RUN = Math.random() < 2 ? 1 : 0;

const board: number[] = new Array<number>(128).fill(0);
for (let i = 0; i < 128; i++) board[i] = ((i * 7) % 13) - 6;

function sign(side: string): number {
  return side === "white" ? 1 : -1;
}

// ---- covered: the sign of a product of two int32 values ----
function rays(square: number, by: string): number {
  const color = sign(by);
  let hits = 0;
  for (const delta of [-17, -16, -15, -1, 1, 15, 16, 17]) {
    let source = square + delta;
    while ((source & 0x88) === 0 && source >= 0) {
      const piece = board[source] ?? 0;
      if (piece !== 0) {
        if (piece * color > 0) hits += Math.abs(piece);
        break;
      }
      source += delta;
    }
  }
  return hits;
}
function own(by: string): number {
  const color = sign(by);
  let n = 0;
  for (let from = 0; from < 128 * RUN; from++) {
    if ((from & 0x88) !== 0) { from += 7; continue; }
    const p = board[from] ?? 0;
    if (p * color <= 0) continue;
    if ((board[from + 1] ?? 0) * color < 0) n += 100;
    n++;
  }
  return n;
}
{
  let a = 0, b = 0;
  for (let s = 0; s < 128 * RUN; s++) { a += rays(s, "white"); b += rays(s, "black"); }
  out.push("rays " + a + "," + b + " own " + own("white") + "," + own("black"));
}

// ---- covered: every relation, the extremes of int32 ----
const EDGE = [-2147483648, -2147483647, -65536, -46341, -46340, -3, -1, 0, 1, 2, 46340, 46341, 65535, 65536, 2147483646, 2147483647];
function relations(): string {
  let lt = 0, le = 0, gt = 0, ge = 0, eq = 0, ne = 0, leq = 0, lne = 0;
  for (let i = 0; i < EDGE.length * RUN; i++) {
    const a = EDGE[i] | 0;
    for (let j = 0; j < EDGE.length; j++) {
      const b = EDGE[j] | 0;
      if (a * b < 0) lt++;
      if (a * b <= 0) le++;
      if (a * b > 0) gt++;
      if (a * b >= 0) ge++;
      if (a * b === 0) eq++;
      if (a * b !== 0) ne++;
      if (a * b == 0) leq++;
      if (a * b != 0) lne++;
    }
  }
  return [lt, le, gt, ge, eq, ne, leq, lne].join(",");
}
out.push("relations " + relations());

// A product past int32 against a small value, and against another int32.
function against(): string {
  let a1 = 0, a2 = 0, a3 = 0, a4 = 0;
  for (let i = 0; i < EDGE.length * RUN; i++) {
    const a = EDGE[i] | 0;
    for (let j = 0; j < EDGE.length; j++) {
      const b = EDGE[j] | 0;
      if (a * b > 2147483647) a1++;
      if (a * b < -2147483648) a2++;
      if (a * b === b) a3++;
      if (a * b >= a) a4++;
    }
  }
  return [a1, a2, a3, a4].join(",");
}
out.push("against " + against());

// Sums and differences that leave int32: an i32 add would wrap.
function sums(): string {
  let s1 = 0, s2 = 0, s3 = 0, s4 = 0, s5 = 0;
  for (let i = 0; i < EDGE.length * RUN; i++) {
    const a = EDGE[i] | 0;
    for (let j = 0; j < EDGE.length; j++) {
      const b = EDGE[j] | 0;
      if (a + b > 2147483647) s1++;
      if (a - b < -2147483648) s2++;
      if (a + b === a - b) s3++;
      if (a + b < a) s4++;
      if (a - b > a + 1) s5++;
    }
  }
  return [s1, s2, s3, s4, s5].join(",");
}
out.push("sums " + sums());

// Bounded operands: the product of two bytes is an int32 (the i32 width),
// of two 20-bit values an exact integer past int32 (the i64 width).
function bounded(seed: number): string {
  let n1 = 0, n2 = 0, n3 = 0, n4 = 0;
  let x = seed | 0;
  for (let i = 0; i < 400 * RUN; i++) {
    x = (Math.imul(x, 1103515245) + 12345) | 0;
    const lo = x & 0xff;
    const hi = (x >> 8) & 0xff;
    const p = x & 0xfffff;
    const q = (x >>> 12) & 0xfffff;
    if (lo * hi > 16000) n1++;
    if (lo * hi + lo - hi === ((lo * hi) | 0) + lo - hi) n2++;
    if (p * q > 274877906944) n3++;
    if (p * q - q * p === 0 && (p + 1) * q !== p * q + 1) n4++;
  }
  return [n1, n2, n3, n4].join(",");
}
out.push("bounded " + bounded(12345));

// Nested trees on both sides.
function nested(): string {
  let n = 0, m = 0;
  for (let i = -6; i <= 6 * RUN; i++) {
    for (let j = -6; j <= 6; j++) {
      const a = i | 0, b = j | 0;
      if ((a + b) * (a - b) <= a * a - b * b) n++;
      if ((a + b) * 3 - a * 2 > b * b - 7) m++;
    }
  }
  return n + "," + m;
}
out.push("nested " + nested());

// Words the facts hold: an int32 element of a class's table.
class Table {
  cells: number[] = [];
  bias = 3;
  constructor() {
    for (let i = 0; i < 64; i++) this.cells.push(((i * 37) % 23) - 11);
  }
  at(k: number): number {
    return this.cells[k] ?? 0;
  }
  count(color: number): number {
    let n = 0;
    for (let k = 0; k < 64 * RUN; k++) {
      if (this.at(k) * color > 0) n++;
      if ((this.cells[k] ?? 0) * color < -4) n += 10;
      if (this.bias * color + this.at(k) >= 0) n += 100;
    }
    return n;
  }
}
const table = new Table();
out.push("table " + table.count(1) + "," + table.count(-1));

// A zero product has a sign no test observes: `-0 > 0`, `-0 === 0`.
function zeros(): string {
  let z = 0, neg = 0, pos = 0;
  for (let i = -3; i <= 3 * RUN; i++) {
    const a = i | 0;
    if (a * 0 === 0) z++;
    if (0 * a < 0) neg++;
    if (a * 0 >= 0) pos++;
  }
  return z + "," + neg + "," + pos;
}
out.push("zeros " + zeros());

// ================= negative controls =================

// The sign of a zero product IS observed by a value use.
function zeroSign(a: number, b: number): string {
  const p = a * b;
  return (1 / p) + "," + Object.is(p, -0) + "," + Object.is(a * b, 0) + "," + (a * b === 0) + "," + (1 / (a * b) < 0);
}
out.push("zero-sign " + zeroSign(0, -5) + " " + zeroSign(-7, 0) + " " + zeroSign(0, 5) + " " + zeroSign(0 * RUN, -1 * RUN));

// Past 2^53: the double product rounds, two different integers compare equal.
function rounding(a: number, b: number, c: number): string {
  const x = a | 0, y = b | 0, z = c | 0;
  const r: (boolean | number)[] = [];
  r.push(x * x === y * z);            // 94906267^2 rounds to 94906266 * 94906268
  r.push(x * x > y * z);
  r.push(x * x - y * z);
  r.push(x * x === x * x + 1);        // the operand of `+` is past 2^53
  r.push(x * x + 1 > x * x);
  r.push(x * x * 3 > x * x * 3 - 1);
  r.push(x * x > 9007199515875288);
  r.push(x * x >= 9007199515875289);
  r.push(x * x > 4294967296);         // rounds, against an exact value: true
  r.push(-x * x < -4294967296);
  return r.join(",");
}
out.push("rounding " + rounding(94906267, 94906266, 94906268) + " | " + rounding(2147483647, 2147483646, -2147483648));

// 2^31 * 2^31 and friends against values at and around 2^53 and 2^62.
function big(a: number, b: number): string {
  const x = a | 0, y = b | 0;
  const p = x * y;
  return [x * y === p, x * y > 4611686014132420608, x * y === 4611686014132420609, x * y < 4611686018427387904, x * y + x * y === 2 * p].join(",");
}
out.push("big " + big(2147483647, 2147483647) + " " + big(-2147483648, -2147483648) + " " + big(-2147483648, 2147483647));

// NaN, Infinity, fractions and non-numbers as operands: no leaf.
function loose(a: any, b: any): string {
  return [a * b > 0, a * b <= 0, a * b === 0, a * b !== 0, a + b > 1, a - b < 0].join(",");
}
out.push("nan " + loose(NaN, 3) + " " + loose(3, NaN) + " " + loose(undefined, 2));
out.push("inf " + loose(Infinity, 0) + " " + loose(-Infinity, 2) + " " + loose(Infinity, -Infinity));
out.push("frac " + loose(0.5, 4) + " " + loose(1.5, -2) + " " + loose(1e-320, 1e-10) + " " + loose(-1e-320, 1e-10));
out.push("other " + loose("3", 2) + " " + loose(null, 5) + " " + loose(true, -2) + " " + loose("x", 1) + " " + loose([4], [2]));
function numbers(a: number, b: number): string {
  return [a * b > 0, a * b <= 0, a * b === 0, a + b > a, a - b === a, a * b < a + b].join(",");
}
out.push("numbers " + numbers(NaN, 3) + " " + numbers(0.25, 8) + " " + numbers(-0, 5) + " " + numbers(1e308, 10) + " " + numbers(9007199254740993, 1) + " " + numbers(3, 4));

// A hole and a past-the-end read are `undefined`: NaN in the product.
const sparse: number[] = [1, -2, 3];
function holes(): string {
  let pos = 0, neg = 0, nan = 0;
  for (let k = 0; k < 6 * RUN; k++) {
    const v = sparse[k];
    if (v * -1 > 0) pos++;
    else if (v * -1 < 0) neg++;
    else nan++;
    if ((sparse[k] ?? 0) * 2 >= 0) pos += 10;
  }
  return pos + "," + neg + "," + nan;
}
out.push("holes " + holes());

// A counter that leaves as a double: its compares may be integer, its value
// is the double's.
function counter(limit: number): string {
  let distance = 1;
  let half = 0;
  let source = 0;
  while (source * 2 < limit) {
    if (distance * 3 > 10) half += distance / 4;
    source += 3;
    distance++;
  }
  return distance + "," + half + "," + distance / 2 + "," + (distance * 0.5 > 3) + "," + source;
}
out.push("counter " + counter(40 * RUN) + " " + counter(0) + " " + counter(-5));
function accumulate(n: number): string {
  let acc = 0;
  let wide = 1;
  for (let i = 1; i <= n; i++) {
    acc += i * i;
    wide *= 3;
    if (acc * 2 > wide) acc -= 1;
  }
  return acc + "," + wide + "," + (wide * wide > acc) + "," + (wide + 1 === wide);
}
out.push("accumulate " + accumulate(10 * RUN) + " " + accumulate(40 * RUN));

// Operands that write or call: evaluated once, left to right.
const log: string[] = [];
function tap(name: string, v: number): number {
  log.push(name);
  return v;
}
function effects(): string {
  let i = 3;
  const r: (boolean | number)[] = [];
  r.push(i++ * i > 11);                 // 3 * 4
  r.push(i * i++ === 16);               // 4 * 4
  r.push((i += 2) * 2 - i === 7);       // 7 * 2 - 7
  r.push(tap("a", 2) * tap("b", -3) < tap("c", 0));
  r.push(tap("d", 5) + tap("e", 1) * tap("f", 2) >= 7);
  r.push(i);
  return r.join(",") + " " + log.join("");
}
out.push("effects " + effects());

// BigInt products keep their own arithmetic.
function bigs(a: bigint, b: bigint): string {
  return [a * b > 0n, a * b === 6n, a + b < a * b, a - b <= 0n].join(",");
}
out.push("bigint " + bigs(2n, 3n) + " " + bigs(-2n, 3n) + " " + bigs(9007199254740993n, 1n));

console.log(out.join("\n"));
