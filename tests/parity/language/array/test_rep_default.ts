// `a[k] ?? <number>` over an Array of Numbers: a present slot is a Number,
// so only a miss (a hole, a key outside the Array, a key that names a
// property) is tested for `undefined` and replaced by the default.
const many = process.argv.length > 100;

// (a) In range, past the end, negative, by a counter and by a wide key.
function sum(xs: number[], from: number, to: number): number {
  let s = 0;
  for (let i = from; i < to; i++) s += xs[i] ?? 100;
  return s;
}
const base: number[] = [1.5, 2, 3, 4, 5.25];
console.log("counter", sum(base, 0, 5), sum(base, 3, 8), sum(base, -2, 2), sum(base, 5, 5));
function down(xs: number[], from: number): number {
  let s = 0;
  let j = from;
  while (j >= 0 && s < 1000) {
    s += xs[j] ?? 0.5;
    j--;
  }
  return s;
}
console.log("wide", down(base, 4), down(base, 7), down(base, 0), down(base, many ? 3 : -1));
function pick(xs: number[], k: number): number {
  return xs[k >>> 0] ?? -1;
}
console.log("uint32", pick(base, 0), pick(base, 4), pick(base, 5), pick(base, -1), pick(base, 4294967296 + 2));

// (b) A hole and a slot past the length read the default; a NaN, a -0 and
// an infinity are Numbers and stay.
const holey: number[] = [1, , 3];
holey[5] = 6;
let hs = "";
for (let i = 0; i < 8; i++) hs += (holey[i] ?? 9) + ",";
console.log("holes", hs);
const edge: number[] = [0, 1, 2, 3];
edge[0] = Number(many ? "1" : "x");
edge[1] = (many ? 3 : -4) * 0;
edge[2] = 1 / 0;
let es = "";
for (let i = 0; i < 5; i++) es += String(edge[i] ?? 7) + ",";
console.log("edge", es, Object.is(edge[1] ?? 7, -0), Number.isNaN(edge[0] ?? 7));

// (c) The default is evaluated as written: a fraction, a negative, zero.
const grown: number[] = [];
for (let i = 0; i < 6; i++) grown.push(i * 1.5);
let gs = 0;
for (let i = 0; i < 9; i++) gs += (grown[i] ?? 0.25) + (grown[i + 4] ?? -2) + (grown[8 - i] ?? 0);
console.log("defaults", gs);

// (d) In a compare, a store and an argument; an insertion sort.
function order(values: readonly number[], weights: readonly number[]): number[] {
  const result = values.slice();
  const scores: number[] = [];
  for (const w of weights) scores.push(w);
  for (let i = 1; i < result.length; i++) {
    const value = result[i] ?? 0;
    const score = scores[i] ?? 0;
    let j = i - 1;
    while (j >= 0 && (scores[j] ?? 0) < score) {
      result[j + 1] = result[j] ?? 0;
      scores[j + 1] = scores[j] ?? 0;
      j--;
    }
    result[j + 1] = value;
    scores[j + 1] = score;
  }
  return result;
}
let acc = 0;
for (let r = 0; r < 200; r++) {
  const vs: number[] = [];
  const ws: number[] = [];
  for (let i = 0; i < 12; i++) {
    vs.push(i + r);
    ws.push((i * 7 + r * 5) % 11);
  }
  const out = order(vs, ws);
  acc += (out[0] ?? 0) + 2 * (out[6] ?? 0) + 3 * (out[11] ?? 0) + (out[12] ?? 1000);
}
console.log("sort", acc, order([4, 5, 6], [1, 3, 2]).join(","));

// (e) Negative controls: an Array that also holds a string, `undefined` or
// `null` keeps the general test; an inherited index is read on the miss.
const mixed: any[] = [1, "two", undefined, null, 5];
let ms = "";
for (let i = 0; i < 6; i++) ms += String(mixed[i] ?? "d") + ",";
console.log("mixed", ms);
const proto: any = [];
proto[1] = 77;
const child: number[] = [10, , 30];
Object.setPrototypeOf(child, proto);
console.log("inherited", child[0] ?? -1, child[1] ?? -1, child[2] ?? -1, child[3] ?? -1);
const named: any = [1, 2];
named[-1] = 5;
named["1.5"] = 6;
function anyKey(xs: any, k: number): any {
  return xs[k] ?? "none";
}
console.log("named", anyKey(named, -1), anyKey(named, 1.5), anyKey(named, 0), anyKey(named, 2), anyKey(named, 4294967295));
