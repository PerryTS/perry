// #13645: for-of over an array whose entry picks the array arm runs the
// indexed loop body. Every observable step must match ArrayIterator.next.
function probe(n: number): number { return n * 2; }

// Typed number[] with a call in the body: the counted array arm.
const nums: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
let total = 0;
for (const x of nums) total += probe(x);
console.log("sum", total);

// Live length: push during iteration is visited, pop shortens.
const grow: number[] = [1, 2, 3];
const seen: number[] = [];
for (const x of grow) { seen.push(x); if (x < 3) grow.push(x + 10); }
console.log("grow", seen.join(","));
const shrink: number[] = [1, 2, 3, 4, 5];
const kept: number[] = [];
for (const x of shrink) { kept.push(probe(x)); shrink.pop(); }
console.log("shrink", kept.join(","));
const cut: number[] = [1, 2, 3, 4];
const before: number[] = [];
for (const x of cut) { before.push(x); if (x === 2) cut.length = 0; }
console.log("cut", before.join(","));
const ahead: number[] = [1, 2, 3];
const late: number[] = [];
for (const x of ahead) { late.push(x); ahead[1] = 20; }
console.log("ahead", late.join(","));

// Holes read through the prototype; a hole on Array.prototype resolves.
const holey: any[] = [1, , 3];
const holes: string[] = [];
for (const x of holey) holes.push(String(x));
(Array.prototype as any)[1] = "P";
for (const x of holey) holes.push(String(x));
delete (Array.prototype as any)[1];
console.log("holes", holes.join(","));

// A declared number[] that holds other values at run time.
const liar: number[] = ["a", 2, null, { v: 1 }] as any;
const told: string[] = [];
for (const x of liar) told.push(typeof x + ":" + String(probe(1)) + JSON.stringify(x));
console.log("liar", told.join("|"));

// Strings and objects.
const words: string[] = ["ab", "cde", ""];
let chars = 0;
for (const w of words) chars += w.length + probe(0);
console.log("words", chars);
const objs = [{ v: 1 }, { v: 2 }, { v: 3 }];
let vs = 0;
for (const o of objs) vs += o.v * probe(1);
console.log("objs", vs);

// break / continue / return / labels / nesting.
function firstOver(a: number[], k: number): number {
  for (const x of a) { if (probe(x) > k) return x; }
  return -1;
}
console.log("ret", firstOver(nums, 9), firstOver(nums, 100));
const pick: number[] = [];
for (const x of nums) { if (x % 2 === 0) continue; if (x > 5) break; pick.push(probe(x)); }
console.log("pick", pick.join(","));
const pairs: string[] = [];
outer: for (const a of [1, 2, 3]) {
  for (const b of nums) {
    if (b === 4) continue outer;
    if (a === 3) break outer;
    pairs.push(a + ":" + b);
  }
}
console.log("labels", pairs.join(","));

// Closures capture each iteration's binding.
const fns: (() => number)[] = [];
for (const x of [7, 8, 9]) fns.push(() => probe(x));
console.log("closures", fns.map((f) => f()).join(","));

// Destructuring and assignment heads use the step value.
const kv: [string, number][] = [["a", 1], ["b", 2]];
const flat: string[] = [];
for (const [k, v] of kv) flat.push(k + probe(v));
let last = 0;
for (last of nums) probe(last);
console.log("heads", flat.join(","), last);

// Throw from the body propagates after one Get per step.
let reads = 0;
const counted = new Proxy([1, 2, 3], {
  get(t, p, r) { if (p === "0" || p === "1" || p === "2") reads++; return Reflect.get(t, p, r); },
});
try { for (const x of counted) { if (x === 2) throw new Error("stop" + probe(x)); } }
catch (e) { console.log("throw", (e as Error).message, reads); }

// An Array subclass without overrides iterates like an array.
class Bag extends Array<number> {}
const bag = new Bag();
bag.push(5, 6, 7);
let bs = 0;
for (const x of bag) bs += probe(x);
console.log("subclass", bs);

// One instance with its own @@iterator takes the protocol.
const own: number[] = [1, 2, 3];
(own as any)[Symbol.iterator] = function* () { yield 42; };
const ownSeen: number[] = [];
for (const x of own) ownSeen.push(probe(x));
for (const x of nums) ownSeen.push(x);
console.log("own", ownSeen.length, ownSeen[0]);
