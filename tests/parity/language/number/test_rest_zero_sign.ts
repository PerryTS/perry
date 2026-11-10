// −0 tolerance (RFC §10.3 as amended, PR D): a Number may be I32 although it
// may be -0 only when no use observes the sign of a zero. Each section makes
// -0 (`0 * negative`, `-x` of 0, `-4 % 2`) and uses it; the positive cases
// only compare, index, mask or take Math.abs (tolerated), the negative
// controls observe the sign (`Object.is`, `1/x`, `x/y`, `%`, Math.sign,
// Math.min, `**`, console, JSON, String, an escape): they must print -0's
// own results, exactly as node does.

const neg = process.argv.length > 100 ? 1 : -1;

// A board of `color * kind`, read through `?? 0`.
class Board {
  cells: number[] = new Array(16).fill(0);
  put(i: number, color: number, kind: number): void {
    this.cells[i & 15] = color * kind;
  }
  at(i: number): number {
    return this.cells[i & 15] ?? 0;
  }
}

// Tolerated: only compares, keys, masks, Math.abs.
const b = new Board();
for (let i = 0; i < 16; i++) b.put(i, i % 3 === 0 ? neg : 1, i & 3);
let empty = 0;
let sum = 0;
const values = [0, 1, 2, 3];
for (let i = 0; i < 16; i++) {
  const p = b.at(i);
  if (p === 0) empty++;
  if (p > 0) sum += 2;
  sum += values[Math.abs(p)] ?? 0;
  sum += p & 1;
  sum += (p + 6) * 3;
}
console.log("tolerated", empty, sum);

// Negative controls on the same kind of value.
const c = new Board();
for (let i = 0; i < 4; i++) c.put(i, neg, i & 1);
const z = c.at(0);
console.log("eq", z === 0, z == 0, -0 === 0);
console.log("is", Object.is(z, -0), Object.is(c.at(2), 0));
console.log("inv", 1 / z, 1 / c.at(2));
console.log("div", z / 5, 5 / z, z / -5);
console.log("mod", (-4 * (neg * neg)) % 2, 1 / ((-4 * (neg * neg)) % 2));
console.log("sign", Math.sign(z), 1 / Math.sign(z), 1 / Math.min(z, 0), 1 / Math.max(z, -1));
console.log("pow", z ** -1, (-z) ** -1);
console.log("neg", 1 / -c.at(1 - 1), 1 / -(neg - neg));
console.log("print", z, [z, c.at(1)], String(z), `${z}`, JSON.stringify([z]));
console.log("cells", c.cells.slice(0, 4), JSON.stringify(c.cells.slice(0, 4)));

// An escape: unknown code reads the slots.
const d = new Board();
for (let i = 0; i < 4; i++) d.put(i, neg, 0);
(globalThis as any).boardEscape = d;
const seen = (globalThis as any).boardEscape as Board;
console.log("escape", Object.is(seen.cells[1], -0), 1 / seen.at(3));

// A tolerated local whose -0 enters an I32 slot, then a non-observing use.
function weigh(a: number, k: number): number {
  const w = a * k;
  return w === 0 ? 10 : (w & 7) + 1;
}
let wsum = 0;
for (let i = 0; i < 6; i++) wsum += weigh(i % 2, neg);
console.log("weigh", wsum);
