// Bind snapshots observable metadata, but immutable body metadata may stay lazy.
function add(this: any, a: number, b: number) { return this.bias + a + b; }
const receiver = { bias: 4 };
const fresh = add.bind(receiver);
Object.defineProperty(add, "name", { value: "changed", configurable: true });
Object.defineProperty(add, "length", { value: 9, configurable: true });
console.log("snapshot", fresh.name, fresh.length, fresh(1, 2));
const changed = add.bind(receiver, 1);
Object.defineProperty(add, "name", { value: 42, configurable: true });
Object.defineProperty(add, "length", { value: -3.5, configurable: true });
console.log("override snapshot", changed.name, changed.length, changed(2));
console.log("nonstring", add.bind(receiver).name, add.bind(receiver).length);
const twice = fresh.bind({ bias: 100 }, 1);
console.log("twice", twice.name, twice.length, twice(2));
Object.defineProperty(fresh, "name", { value: "own", configurable: true });
Object.defineProperty(fresh, "length", { value: 13, configurable: true });
console.log("bound override", fresh.bind(receiver, 1).name, fresh.bind(receiver, 1).length);
console.log("source", twice.toString());

const reads: string[] = [];
function observed(a: number, b: number) { return a + b; }
Object.defineProperty(observed, "length", {
  get() { reads.push("length"); return 8.9; }, configurable: true,
});
Object.defineProperty(observed, "name", {
  get() { reads.push("name"); return "visible"; }, configurable: true,
});
const ob = observed.bind(null, 3);
console.log("getters at bind", reads.join(","));
console.log("getters result", ob.name, ob.length, ob(4), reads.join(","));
Object.defineProperty(observed, "name", {
  get() { throw new Error("name getter"); }, configurable: true,
});
try { observed.bind(null); } catch (e: any) { console.log("throws at bind", e.message); }

class Pair { sum: number; constructor(a: number, b: number) { this.sum = a + b; } }
const BoundPair: any = Pair.bind(null, 5);
const pair = new BoundPair(6);
console.log("construct", pair.sum, pair instanceof Pair, pair instanceof BoundPair, BoundPair.name, BoundPair.length);

function bindSite(f: any, r: any) { return f.bind(r); }
function method(this: any) { return this.bias; }
for (let i = 0; i < 100; i++) bindSite(method, receiver)();
const original = Function.prototype.bind;
const patch = function(this: any, r: any) { return () => r.bias + 100; };
(Function.prototype as any).bind = patch;
const patched = bindSite(method, receiver);
(Function.prototype as any).bind = original;
console.log("patched after warmup", patched(), bindSite(method, receiver)());
