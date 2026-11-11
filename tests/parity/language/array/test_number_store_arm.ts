// The number-over-number arm of a guarded element store: a Number stored
// over a Number changes the slot only. Every other pair (a hole, a pointer
// or a string on either side, a growing store, a frozen Array, an inherited
// setter) must keep the general store, exactly as in JavaScript.
const many = process.argv.length > 100;

// (a) Numbers over Numbers through a key that is no proven index.
function put(xs: number[], k: number, v: number): number {
  xs[k] = v;
  return xs.length;
}
const nums: number[] = [1.5, 2, 3, 4];
console.log("over number", put(nums, 0, 9.25), put(nums, 3, -7), put(nums, many ? 1 : 2, 1e300), nums.join(","));

// (b) NaN, -0 and infinities are Numbers; -0 stays -0.
const edge: number[] = [1, 2, 3, 4, 5];
put(edge, 0, Number(many ? "1" : "x"));
put(edge, 1, (many ? 3 : -4) * 0);
put(edge, 2, 1 / 0);
put(edge, 3, -1 / 0);
put(edge, 4, 0 / (many ? 1 : 0));
console.log("edge", edge.join(","), Object.is(edge[1], -0), Number.isNaN(edge[0]), Number.isNaN(edge[4]), edge.indexOf(NaN), edge.includes(NaN));

// (c) A hole: the store fills it, below and past the length.
const holey: number[] = [1, , 3];
console.log("hole", 1 in holey, put(holey, 1, 20), 1 in holey, put(holey, 5, 60), 4 in holey, holey.join("|"), holey.length);
const fresh: number[] = new Array(4);
for (let i = 3; i >= 0; i--) put(fresh, i >>> 0, i * 1.5);
console.log("fresh", fresh.join(","), fresh.length);

// (d) Growth: at the length and past it.
const grow: number[] = [];
let lens = "";
for (let i = 0; i < 20; i++) lens += put(grow, i >>> 0, i * i) + ",";
console.log("grow", lens, grow[19], put(grow, 40, 1), grow.length, 30 in grow);

// (e) A slot that holds a string or an object: the Number replaces it, and
// a string or an object replaces a Number.
function putAny(xs: any[], k: number, v: any): any {
  xs[k] = v;
  return xs[k];
}
const mixed: any[] = ["s", { a: 1 }, 3, null, undefined, true, 7n];
function putNum(xs: any[], k: number, v: number): any {
  xs[k] = v;
  return xs[k];
}
let over = "";
for (let i = 0; i < 7; i++) over += String(putNum(mixed, i >>> 0, i + 0.5)) + ",";
console.log("over mixed", over, mixed.join("|"));
const back: any[] = [1, 2, 3, 4];
console.log("number replaced", putAny(back, 0, "str"), JSON.stringify(putAny(back, 1, { b: 2 })), putAny(back, 2, null), putNum(back, 0, 5), putNum(back, 1, 6), back.join("|"));
// Objects stored over Numbers stay alive across collections.
const keep: any[] = [0, 0, 0, 0, 0, 0, 0, 0];
for (let r = 0; r < 2000; r++) {
  putNum(keep, (r & 7) >>> 0, r);
  putAny(keep, ((r + 3) & 7) >>> 0, { r, pad: [r, r + 1] });
}
let alive = 0;
for (const k of keep) alive += typeof k === "number" ? k : k.r + k.pad[1];
console.log("alive", alive);

// (f) A frozen Array rejects the store; a sealed one takes it.
const frozen: number[] = Object.freeze([1, 2, 3]) as number[];
let threw = "no";
try {
  put(frozen, 1, 99);
} catch (e) {
  threw = e instanceof TypeError ? "TypeError" : "other";
}
const sealed: number[] = Object.seal([1, 2, 3]);
console.log("integrity", threw, frozen.join(","), put(sealed, 1, 99), sealed.join(","));

// (g) A setter on the prototype sees a hole fill, never an overwrite.
let seen = "";
const proto: any = [];
Object.defineProperty(proto, "1", {
  set(v: number) {
    seen += "set" + v + ";";
  },
  get() {
    return -1;
  },
  configurable: true,
});
const child: any = [10, , 30];
Object.setPrototypeOf(child, proto);
function putChild(xs: any, k: number, v: number): any {
  xs[k] = v;
  return xs[k];
}
console.log("inherited", putChild(child, 1, 5), putChild(child, 0, 6), putChild(child, 2, 7), seen, Object.keys(child).join(","));

// (h) The insertion sort over a copy and a pushed Array.
function order(values: readonly number[], weights: readonly number[]): number[] {
  const result = values.slice();
  const scores: number[] = [];
  for (const w of weights) scores.push(w * 1.5);
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
for (let r = 0; r < 300; r++) {
  const vs: number[] = [];
  const ws: number[] = [];
  for (let i = 0; i < 16; i++) {
    vs.push(i * 2 + r);
    ws.push((i * 5 + r * 7) % 13);
  }
  const out = order(vs, ws);
  acc += (out[0] ?? 0) + 3 * (out[7] ?? 0) + 5 * (out[15] ?? 0);
}
console.log("sort", acc, order([1, 2, 3], [1, 3, 2]).join(","));

// (i) An Array with a generic reader, and a typed array (untouched).
const shared: number[] = [4, 5, 6];
put(shared, 1, 50);
console.log("generic", JSON.stringify(shared), Object.entries(shared).join(";"));
const ta = new Float64Array([1, 2, 3]);
function putTa(t: Float64Array, k: number, v: number): number {
  t[k] = v;
  return t[k] ?? -1;
}
console.log("typed", putTa(ta, 1, 2.5), putTa(ta, 5, 9), ta.join(","));
