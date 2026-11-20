// #4099 — installing the `size` accessor on Map.prototype / Set.prototype must
// not corrupt a sibling data method's slot. Pre-fix, the `size` getter was
// installed *after* the data methods, and the accessor install clobbered the
// `Map.prototype.set` data slot with a garbage number — so `typeof
// Map.prototype.set` was "number" and `Map.prototype.set.call(...)` crashed
// (SIGBUS) by dereferencing the number as an object pointer.

console.log("typeof Map.set:", typeof Map.prototype.set);
console.log("typeof Map.get:", typeof Map.prototype.get);
console.log("typeof Set.add:", typeof Set.prototype.add);
console.log("typeof Map.size getter:", typeof Object.getOwnPropertyDescriptor(Map.prototype, "size")!.get);

function threw(fn: () => void): string {
    try {
        fn();
        return "NO_THROW";
    } catch (e: any) {
        return e && e.name ? e.name : String(e);
    }
}

// Reflective set on a bad receiver must throw (used to crash).
console.log("set(null):", threw(() => (Map.prototype.set as any).call(null, 1, 2)));
console.log("set({}):", threw(() => (Map.prototype.set as any).call({}, 1, 2)));

// The accessor and the data methods both still work on a real instance.
const m = new Map<string, number>();
(Map.prototype.set as any).call(m, "k", 7);
console.log("reflective set then get:", m.get("k"), "size:", m.size);

const s = new Set<number>();
s.add(1);
s.add(2);
console.log("set size:", s.size, "has(1):", s.has(1));

// Map constructed from an iterable still initializes via the set fast-path.
const m2 = new Map([
    ["x", 10],
    ["y", 20],
]);
console.log("map ctor:", m2.get("x"), m2.get("y"), "size:", m2.size);
