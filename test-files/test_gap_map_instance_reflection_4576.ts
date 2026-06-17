// Map instance/prototype reflection + live forEach (built-ins/Map test262).
//
// Three fixes verified here:
//   1. Inherited Map.prototype members read off a Map *instance* resolve
//      through the prototype chain — `m.set`, `m.get`, `m.constructor` were
//      previously `undefined` on an instance (only `.size` was special-cased),
//      so `m.set.call(m, k, v)` threw "not a function" and
//      `(new Map()).constructor === Map` was false.
//   2. `Map.prototype.forEach` iterates [[MapData]] live: entries appended
//      during the callback must be visited (re-read size each step).
//   3. `delete obj[Symbol.iterator]` actually removes the symbol property
//      (was reported as success but left the property in place).

// --- (1) instance inherits prototype members -------------------------------
const m = new Map<string, number>();
console.log("typeof m.set:", typeof m.set);
console.log("m.set === proto.set:", m.set === Map.prototype.set);
console.log("inst.constructor === Map:", (new Map()).constructor === Map);
console.log("Map.prototype.constructor === Map:", Map.prototype.constructor === Map);

// reflective set via the instance-resolved method, plus chaining
const r = new Map<string, number>();
(r.set as any).call(r, "x", 9);
console.log("reflective x:", r.get("x"));
r.set("a", 1).set("b", 2);
console.log("chain a,b:", r.get("a"), r.get("b"));

// --- (2) forEach visits entries added during iteration ---------------------
const live = new Map<string, number>();
live.set("foo", 0);
live.set("bar", 1);
let count = 0;
const seen: string[] = [];
live.forEach((v, k) => {
    if (count === 0) live.set("baz", 2);
    seen.push(k);
    count++;
});
console.log("forEach count:", count, "seen:", seen.join(","));

// delete during forEach (compaction-based delete stays correct here)
const del = new Map<string, number>();
del.set("foo", 0);
del.set("bar", 1);
let dc = 0;
const dseen: string[] = [];
del.forEach((_v, k) => {
    if (dc === 0) del.delete("bar");
    dseen.push(k);
    dc++;
});
console.log("forEach-del count:", dc, "seen:", dseen.join(","));

// --- (3) Symbol.iterator descriptor is configurable (delete works) ---------
const hasBefore = Object.prototype.hasOwnProperty.call(Map.prototype, Symbol.iterator);
const desc = Object.getOwnPropertyDescriptor(Map.prototype, Symbol.iterator)!;
console.log("symiter configurable:", desc.configurable, "enumerable:", desc.enumerable);
console.log("symiter === entries:", Map.prototype[Symbol.iterator] === Map.prototype.entries);
