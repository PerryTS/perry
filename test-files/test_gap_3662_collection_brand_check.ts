// #3662 — built-in collection prototype methods must perform the spec `this`
// brand check and throw a TypeError on an incompatible receiver, and must
// actually work when invoked reflectively on a real collection.
//
// Pre-fix, `Set.prototype.add` & friends resolved to a shared no-op thunk:
// reflective calls silently did nothing and never threw. This pins both the
// brand-check throw (wrong `this` -> TypeError) and the now-working reflective
// dispatch on a correct receiver. We print only error *types* / booleans so
// the output is byte-identical to Node (engine error messages differ).

function threw(fn: () => void): string {
    try {
        fn();
        return "NO_THROW";
    } catch (e: any) {
        return e && e.name ? e.name : String(e);
    }
}

// --- Brand check: wrong / primitive receiver must throw TypeError. ---
console.log("Set.add{}:", threw(() => (Set.prototype.add as any).call({}, 1)));
console.log("Set.has undef:", threw(() => (Set.prototype.has as any).call(undefined, 1)));
console.log("Set.delete 5:", threw(() => (Set.prototype.delete as any).call(5, 1)));
console.log("Set.forEach str:", threw(() => (Set.prototype.forEach as any).call("x", () => {})));
console.log("Map.get{}:", threw(() => (Map.prototype.get as any).call({}, 1)));
console.log("Map.set null:", threw(() => (Map.prototype.set as any).call(null, 1, 2)));
console.log("Map.has{}:", threw(() => (Map.prototype.has as any).call({}, 1)));
console.log("WeakSet.add{}:", threw(() => (WeakSet.prototype.add as any).call({}, {})));
console.log("WeakSet.has 7:", threw(() => (WeakSet.prototype.has as any).call(7, {})));
console.log("WeakMap.get{}:", threw(() => (WeakMap.prototype.get as any).call({}, {})));
console.log("WeakMap.set undef:", threw(() => (WeakMap.prototype.set as any).call(undefined, {}, 1)));

// Cross-brand: a Set is not a Map (and vice-versa) -> TypeError.
console.log("Map.get on Set:", threw(() => (Map.prototype.get as any).call(new Set(), 1)));
console.log("Set.add on Map:", threw(() => (Set.prototype.add as any).call(new Map(), 1)));
console.log("WeakMap.get on WeakSet:", threw(() => (WeakMap.prototype.get as any).call(new WeakSet(), {})));

// --- Correct receiver: reflective dispatch must actually work. ---
const s = new Set<number>();
(Set.prototype.add as any).call(s, 42);
console.log("reflective Set.add -> has(42):", (Set.prototype.has as any).call(s, 42));
console.log("reflective Set.delete:", (Set.prototype.delete as any).call(s, 42));
console.log("after delete has(42):", s.has(42));

const m = new Map<string, number>();
(Map.prototype.set as any).call(m, "k", 7);
console.log("reflective Map.get:", (Map.prototype.get as any).call(m, "k"));
console.log("reflective Map.has:", (Map.prototype.has as any).call(m, "k"));

const wm = new WeakMap<object, number>();
const key = {};
(WeakMap.prototype.set as any).call(wm, key, 99);
console.log("reflective WeakMap.get:", (WeakMap.prototype.get as any).call(wm, key));
console.log("reflective WeakMap.has:", (WeakMap.prototype.has as any).call(wm, key));

const ws = new WeakSet<object>();
const v = {};
(WeakSet.prototype.add as any).call(ws, v);
console.log("reflective WeakSet.has:", (WeakSet.prototype.has as any).call(ws, v));

// --- Sanity: the fast direct-call path is unchanged. ---
const s2 = new Set<number>();
s2.add(1).add(2).add(2);
console.log("direct Set size:", s2.size, "has(1):", s2.has(1));
