// A VALUE read of a WeakMap/WeakSet method (`w.add`, `wm.set`, `typeof w.has`)
// must return the callable prototype method — not `undefined`. Method CALLS
// (`w.add(k)`) already dispatched via the native weak arms, but a bare property
// read had no equivalent, so `const f = w.add` / `w.add.bind(w, k)` yielded
// `undefined` and the subsequent call/bind threw "Bind must be called on a
// function". react-server-dom's chunk-preload dedup does exactly
// `u.add.bind(u, chunk)` on a module-level WeakSet, which 500'd every Next.js
// App Router dynamic route (#5989).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

const ws: any = new WeakSet();
console.log(typeof ws.add, typeof ws.has, typeof ws.delete);

const k1 = {};
const boundAdd = ws.add.bind(ws, k1);
boundAdd();
console.log(ws.has(k1));

// method value stored then invoked
const addFn = ws.add;
const k2 = {};
addFn.call(ws, k2);
console.log(ws.has(k2));

const wm: any = new WeakMap();
console.log(typeof wm.set, typeof wm.get, typeof wm.has, typeof wm.delete);

const mk = {};
const boundSet = wm.set.bind(wm);
boundSet(mk, 42);
console.log(wm.get(mk), wm.has(mk));

// the bind value is a real function object (has .name / .call)
console.log(typeof ws.add.bind, typeof wm.get.call);

// method CALL path still works (regression guard)
const ws2: any = new WeakSet();
const k3 = {};
ws2.add(k3);
console.log(ws2.has(k3), ws2.delete(k3), ws2.has(k3));

// an OWN property shadowing a method name still wins (own-key precedence)
const wm2: any = new WeakMap();
Object.defineProperty(wm2, "get", { value: 123, enumerable: true, configurable: true });
console.log(wm2.get);
