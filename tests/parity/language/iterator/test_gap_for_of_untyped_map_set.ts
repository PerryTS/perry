// Test: `for...of` over a receiver whose static type can NOT be proven
// (an `any`-typed Map/Set, an untyped JS-source value). The for-of desugar
// reads `__arr.length` / `__arr[i]` and so assumes a plain Array; when the
// receiver is actually a Map/Set the loop read `.length` off the wrong
// handle (→ 0) and iterated zero times. The fix routes unproven receivers
// through the runtime default iterator (`js_for_of_to_array`): a Map yields
// `[k, v]` pairs (=== `map.entries()`), a Set yields values, an Array is
// returned unchanged, a string yields code-point chars, and anything else
// drives its `[Symbol.iterator]`. This is the last `effect` Context/Layer
// blocker (#321): effect iterates `for (const [tag, s] of self.unsafeMap)`
// over an untyped Map. Validated byte-for-byte against
// `node --experimental-strip-types`.

// --- untyped Map: simple binding yields [k, v] pairs ---
const obj: any = { m: new Map([["k", 1], ["j", 2]]) };
let n = 0;
for (const e of obj.m) n++;
console.log("untyped map count:", n); // 2

// --- untyped Map: destructuring [k, v] ---
const o2: any = { m: new Map([["a", 1], ["b", 2]]) };
const pairs: string[] = [];
for (const [k, v] of o2.m) pairs.push(`${k}=${v}`);
console.log("map pairs:", pairs.join(",")); // a=1,b=2

// --- untyped Set ---
const s: any = { set: new Set([10, 20, 30]) };
let sum = 0;
for (const x of s.set) sum += x;
console.log("untyped set sum:", sum); // 60

// --- explicit .entries() still works (regression guard) ---
let e2 = 0;
for (const e of obj.m.entries()) e2++;
console.log("explicit entries count:", e2); // 2

// --- typed Map / Set fast paths still work (regression guard) ---
const tm: Map<string, number> = new Map([["x", 1]]);
let tc = 0;
for (const e of tm) tc++;
console.log("typed map count:", tc); // 1

const ts: Set<number> = new Set([1, 2, 3, 4]);
let tsc = 0;
for (const x of ts) tsc++;
console.log("typed set count:", tsc); // 4

// --- untyped string iterates by code point ---
const sv: any = "ab😀c";
const chars: string[] = [];
for (const c of sv) chars.push(c);
console.log("string chars:", chars.join("|"), chars.length); // a|b|😀|c 4

// --- untyped value carrying a custom [Symbol.iterator] ---
const custom: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next: () => (i < 3 ? { value: i++, done: false } : { value: undefined, done: true }),
    };
  },
};
let cs = 0;
for (const x of custom) cs += x;
console.log("custom iter sum:", cs); // 3

// --- generator object held in an `any` ---
function* g() {
  yield 100;
  yield 200;
}
const gv: any = g();
let gs = 0;
for (const x of gv) gs += x;
console.log("gen sum:", gs); // 300

// --- empty untyped Map iterates zero times ---
const em: any = new Map();
let ec = 0;
for (const e of em) ec++;
console.log("empty map count:", ec); // 0

// --- array fast paths unchanged ---
let al = 0;
for (const x of [1, 2, 3]) al += x;
console.log("array literal sum:", al); // 6

const aa: any = [4, 5, 6];
let aas = 0;
for (const x of aa) aas += x;
console.log("untyped array sum:", aas); // 15
