// Refs #1766: WeakMap / WeakSet / WeakRef / FinalizationRegistry wrappers
// previously exposed their backing-storage field names (`entries`, `target`,
// `callback`) as user-visible properties — `(wm as any).entries` returned the
// internal `[key, value]` pair array instead of `undefined` like Node. The
// runtime now uses `__perry_*` sentinel names for those slots so user-level
// property reads land on undefined and the functional WeakMap/WeakSet/WeakRef
// methods continue to work through the same internal accessors.

// === WeakMap ===
const wm = new WeakMap<object, number>();
const k1 = {};
const k2 = {};
wm.set(k1, 11);
wm.set(k2, 22);

// Internal field is hidden.
console.log((wm as any).entries);

// Public API still works.
console.log(wm.has(k1), wm.has(k2));
console.log(wm.get(k1), wm.get(k2));
wm.delete(k1);
console.log(wm.has(k1), wm.has(k2));

// === WeakSet ===
const ws = new WeakSet<object>();
ws.add(k1);
ws.add(k2);

console.log((ws as any).entries);
console.log(ws.has(k1), ws.has(k2));
ws.delete(k1);
console.log(ws.has(k1), ws.has(k2));

// === WeakRef ===
const wr = new WeakRef(k1);
console.log((wr as any).target);
console.log(wr.deref() === k1);

// === FinalizationRegistry ===
const fr = new FinalizationRegistry((_held: any) => {});
console.log((fr as any).callback);
console.log((fr as any).entries);

const tok = {};
fr.register(k1, "h1", tok);
// unregister returns true when an entry matched the token (Node) — Perry now
// agrees because the lookup hits the sentinel-named slot.
console.log(fr.unregister(tok));
