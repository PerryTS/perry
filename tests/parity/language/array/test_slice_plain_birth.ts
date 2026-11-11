// A `slice` of an Array bears a plain Array: the copy has an array rep, so
// its reads and stores take the rep-keyed tiers. The copy is its own Array
// (a store into it never reaches the source), keeps the source's holes, and
// is whatever ArraySpeciesCreate makes when the species is not `%Array%`.
const many = process.argv.length > 100;

// (a) A copy sorted in place; the source is untouched.
function sorted(values: readonly number[]): number[] {
  const out = values.slice();
  for (let i = 1; i < out.length; i++) {
    const v = out[i] ?? 0;
    let j = i - 1;
    while (j >= 0 && (out[j] ?? 0) > v) {
      out[j + 1] = out[j] ?? 0;
      j--;
    }
    out[j + 1] = v;
  }
  return out;
}
const src: number[] = [5, 3, 9, 1, 7, 2, 8];
console.log("sorted", sorted(src).join(","), src.join(","));
let total = 0;
for (let r = 0; r < 300; r++) {
  const vs: number[] = [];
  for (let i = 0; i < 14; i++) vs.push((i * 37 + r * 11) % 23);
  const s = sorted(vs);
  total += (s[0] ?? 0) + 2 * (s[6] ?? 0) + 3 * (s[13] ?? 0) + (vs[0] ?? 0);
}
console.log("many", total);

// (b) Ranges, negative bounds and an empty copy; the copy grows on its own.
const base: number[] = [10, 20, 30, 40, 50];
const mid = base.slice(1, 4);
const tail = base.slice(-2);
const none = base.slice(5);
mid.push(99);
mid[0] = -1;
none.push(1);
tail[many ? 0 : 1] = 0.5;
console.log("ranges", mid.join(","), tail.join(","), none.join(","), base.join(","), mid.length, tail.length);

// (c) Holes are kept; a fraction, a string and an object make the copy's
// elements what they are.
const holey: any[] = [1, , 3, , 5];
const hc = holey.slice();
console.log("holes", 1 in hc, 3 in hc, hc.length, hc.join("|"), (hc[1] = 2), 1 in hc, 1 in holey);
const mixed: any[] = [1, "two", { three: 3 }, 4.5];
const mc = mixed.slice(1);
mc[0] = 7;
console.log("mixed", mc.length, mc[0], JSON.stringify(mc[1]), mc[2], mixed[1]);

// (d) The copy read by a key that may be anything.
function at(xs: number[], k: number): any {
  return xs[k];
}
const copy = base.slice();
console.log("keys", at(copy, 0), at(copy, 4), at(copy, 5), at(copy, -1), at(copy, 1.5), at(copy, 4294967295));

// (e) Integrity and a generic reader on the copy.
const frozen = Object.freeze(base.slice());
let threw = "no";
try {
  (frozen as number[])[0] = 1;
} catch (e) {
  threw = e instanceof TypeError ? "TypeError" : "other";
}
console.log("frozen", threw, frozen.join(","), JSON.stringify(base.slice(0, 2)), Object.keys(base.slice(3)).join(","));

// (f) A species that is not `%Array%`.
const odd: any = [0, 1, 2, 3, 4, 5, 6, 7];
odd.constructor = { [Symbol.species]: Object };
const made = odd.slice(2, 6);
let ms = 0;
for (let i = 0; i < 4; i++) ms += made[i];
console.log("species", Array.isArray(made), typeof made, made[0], made[3], made[4], ms);
const fn: any = [0, 1, 2, 3];
fn.constructor = {
  [Symbol.species]: function (n: number) {
    return Object.freeze(new Array(n));
  },
};
let fthrew = "no";
try {
  fn.slice(1);
} catch (e) {
  fthrew = e instanceof TypeError ? "TypeError" : "other";
}
console.log("frozen species", fthrew);
class Stack extends Array<number> {
  top(): number {
    return this[this.length - 1] ?? -1;
  }
}
const st = new Stack();
st.push(4, 5, 6);
const sc = st.slice(1);
console.log("subclass", sc instanceof Stack, sc.length, sc.top(), Array.isArray(sc));
