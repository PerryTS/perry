// Wide keys: an element read or store whose key the whole-program facts
// prove a non-negative integer Number that may lie past the int32 range
// takes no key normalizer: the key is clamped and converted, and one
// unsigned compare against the Array decides. A key at or past 2^31 is not
// an element of the fast tier, and one at or past 2^32 - 1 is no array
// index at all: each must name what it names in JavaScript.
const many = process.argv.length > 100;

// (a) The insertion sort: `j` is bounded below by the loop test only.
function sortDesc(values: readonly number[], keys: readonly number[]): number[] {
  const result = values.slice();
  const scores: number[] = [];
  for (const k of keys) scores.push(k);
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
console.log("sort", sortDesc([1, 2, 3, 4, 5, 6, 7], [3, 9, 1, 9, 4, 0, 7]).join(","));
console.log("sort", sortDesc([10, 20], [1, 2]).join(","), sortDesc([5], [1]).join(","), sortDesc([], []).join(","));
let total = 0;
for (let r = 0; r < 200; r++) {
  const vs: number[] = [];
  const ks: number[] = [];
  for (let i = 0; i < 12; i++) {
    vs.push(i + r);
    ks.push((i * 7 + r * 3) % 11);
  }
  const out = sortDesc(vs, ks);
  total += (out[0] ?? 0) * 3 + (out[5] ?? 0) * 5 + (out[11] ?? 0);
}
console.log("sort many", total);

// (b) A uint32 key: `>>> 0` of a negative Number is past 2^31.
const table: any = [10, 20, 30, 40];
table[4294967295] = "max";
table["4294967296"] = "two32";
table[9007199254740992] = "two53";
function u(xs: any, k: number): any {
  return xs[k >>> 0];
}
console.log("uint32", u(table, 0), u(table, 3), u(table, 4), u(table, -1), u(table, -2), u(table, 2147483648), u(table, 4294967297));
function putU(xs: any, k: number, v: any): number {
  xs[k >>> 0] = v;
  return xs.length;
}
const grown: any = [1, 2];
console.log("uint32 store", putU(grown, 1, "b"), putU(grown, 5, "f"), putU(grown, -1, "p"), grown[1], grown[5], grown[4294967295], 3 in grown, Object.keys(grown).join(","));

// (c) A key joined over literal callers up to 2^53: 2^32 - 2 is the last
// array index (absent here), 2^32 - 1, 2^32 and 2^53 are named properties.
function rd(xs: any, k: number): any {
  return xs[k];
}
console.log(
  "wide read",
  rd(table, 0),
  rd(table, 3),
  rd(table, 2147483647),
  rd(table, 2147483648),
  rd(table, 4294967294),
  rd(table, 4294967295),
  rd(table, 4294967296),
  rd(table, 9007199254740992),
);
function wr(xs: any, k: number, v: any): number {
  xs[k] = v;
  return xs.length;
}
const sink: any = [1, 2, 3];
console.log(
  "wide store",
  wr(sink, 0, "z"),
  wr(sink, 4, "e"),
  wr(sink, 4294967295, "max"),
  wr(sink, 4294967296, "two32"),
  wr(sink, 9007199254740992, "two53"),
  sink[0],
  sink[3],
  sink[4],
  sink["4294967295"],
  sink["4294967296"],
  sink["9007199254740992"],
  Object.keys(sink).join(","),
);

// (d) A counter that climbs past every bound: the Array answers undefined.
function climb(xs: number[], start: number, step: number, n: number): number {
  let k = start;
  let hits = 0;
  let c = n;
  while (k >= 0 && c > 0) {
    if (xs[k] !== undefined) hits++;
    k = k + step;
    c--;
  }
  return hits * 1000 + (k % 1000);
}
const short: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
console.log("climb", climb(short, 0, 1, 20), climb(short, 5, 2147483647, 6), climb(short, many ? 1 : 7, 1073741824, 40));

// (e) Negative controls the facts refuse: a fraction, NaN, -0 and a key
// that may be negative keep the exact key.
const named: any = [1, 2, 3];
named["0.5"] = "half";
named["NaN"] = "nan";
named[-1] = "minus one";
function anyKey(xs: any, k: number): any {
  return xs[k];
}
const zero = (many ? 3 : -4) * 0;
console.log("exact", anyKey(named, 0.5), anyKey(named, Number(many ? "1" : "x")), anyKey(named, zero), Object.is(zero, -0), anyKey(named, -1), anyKey(named, 2));
function down(xs: any, from: number): string {
  let out = "";
  for (let j = from >>> 0; j > -3; j--) out += String(xs[j]) + ",";
  return out;
}
console.log("may be negative", down(named, 2));

// (f) A holey Array and one with a generic reader, under wide keys.
const holey: any[] = [1, , 3, , 5];
function fill(xs: any[], k: number, v: any): number {
  xs[k >>> 0] = v;
  return xs.length;
}
console.log("holey", u(holey, 1), 1 in holey, fill(holey, 1, "h"), 1 in holey, fill(holey, 3, 9), holey.join("|"), fill(holey, 7, "t"), 6 in holey, holey.length);
const shared: number[] = [4, 5, 6];
function generic(x: any): string {
  return JSON.stringify(x) + Object.keys(x).length;
}
console.log("generic reader", fill(shared, 2, 60), fill(shared, 3, 70), generic(shared), u(shared, 3), u(shared, 4));
