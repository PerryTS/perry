// Gap test for #5588 — Object.assign with a PROXY source.
// A Proxy's NaN-box payload is a small registry id, not a heap ObjectHeader
// pointer, so the raw keys_array walk used to skip it silently: no traps fired
// and a throwing ownKeys/getOwnPropertyDescriptor never propagated. Spec drives
// the source through [[OwnPropertyKeys]] -> [[GetOwnProperty]] (enumerable test)
// -> [[Get]], propagating any abrupt completion (test262
// built-ins/Object/assign/source-own-prop-error + source-own-prop-keys-error).
// Compared byte-for-byte against `node --experimental-strip-types`.

function thrown(fn: () => void): string {
  try { fn(); return "no throw"; } catch (e: any) { return e.constructor.name; }
}

// ---- throwing ownKeys trap propagates ----
const ownKeysThrows = new Proxy({}, {
  ownKeys() { throw new TypeError("ownKeys"); },
});
console.log(thrown(() => Object.assign({}, ownKeysThrows)));          // TypeError

// ---- throwing getOwnPropertyDescriptor trap propagates ----
const gopdThrows = new Proxy({ attr: null }, {
  getOwnPropertyDescriptor() { throw new TypeError("gopd"); },
});
console.log(thrown(() => Object.assign({}, gopdThrows)));            // TypeError

// ---- throwing get trap propagates ----
const getThrows = new Proxy({ a: 1 }, {
  get() { throw new RangeError("get"); },
});
console.log(thrown(() => Object.assign({}, getThrows)));            // RangeError

// ---- plain proxy source: enumerable own props copied via the traps ----
const plain = new Proxy({ a: 1, b: 2 }, {});
console.log(JSON.stringify(Object.assign({}, plain)));             // {"a":1,"b":2}

// ---- non-enumerable own props are skipped (descriptor enumerable:false) ----
const hidden: any = {};
Object.defineProperty(hidden, "secret", { value: 9, enumerable: false });
hidden.shown = 1;
console.log(JSON.stringify(Object.assign({}, new Proxy(hidden, {})))); // {"shown":1}

// ---- get trap actually sources the values ----
const doubler = new Proxy({ x: 2, y: 3 }, {
  get(t: any, k) { return typeof t[k] === "number" ? t[k] * 2 : t[k]; },
});
console.log(JSON.stringify(Object.assign({}, doubler)));          // {"x":4,"y":6}
