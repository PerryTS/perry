// Issue #1757/#1758: WeakMap/WeakSet methods must dispatch when the receiver
// flows through an `any`-typed binding (not a directly-declared
// `const w = new WeakMap()` local). This is the shape effect uses via
// `globalValue(() => new WeakMap())`, whose generic return type erases the
// WeakMap-ness so `.has(...)` lands in the dynamic method dispatcher. Before
// the fix this threw `TypeError: has is not a function`.
//
// Expected output:
// wm has(a): false
// wm set/has(a): true
// wm get(a): 1
// wm has(b): false
// wm delete(a): true
// wm has(a) after delete: false
// ws has(a): false
// ws add/has(a): true
// ws delete(a): true
// ws has(a) after delete: false

// `any`-typed binding defeats static WeakMap tracking → dynamic dispatch.
const wm: any = new WeakMap<object, number>();
const a = {};
const b = {};
console.log("wm has(a): " + wm.has(a));
wm.set(a, 1);
console.log("wm set/has(a): " + wm.has(a));
console.log("wm get(a): " + wm.get(a));
console.log("wm has(b): " + wm.has(b));
console.log("wm delete(a): " + wm.delete(a));
console.log("wm has(a) after delete: " + wm.has(a));

const ws: any = new WeakSet<object>();
console.log("ws has(a): " + ws.has(a));
ws.add(a);
console.log("ws add/has(a): " + ws.has(a));
console.log("ws delete(a): " + ws.delete(a));
console.log("ws has(a) after delete: " + ws.has(a));
