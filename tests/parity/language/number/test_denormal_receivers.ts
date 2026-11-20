// A JS Number whose IEEE bits look like a raw heap pointer (every denormal
// double, and +0-adjacent tiny numbers) must be treated as a Number, never as
// an object. Perry NaN-boxes doubles; a legacy "raw bitcast pointer" shape is a
// word whose top 16 bits are clear, which every denormal also is. Reading or
// writing a property on such a value, or using it as a Map/Set key, used to
// mask its bits into a forged heap pointer and dereference it (type confusion /
// SIGSEGV). These must all match Node: a denormal is a primitive number.
//
// Values exercised: Number.MIN_VALUE * 2^k sweeps the low address range that
// collides with the raw-pointer shape; the largest denormal; +0 / -0; negative
// denormals; a normal number as a control.

function mk(k: number): any {
  return Number.MIN_VALUE * Math.pow(2, k);
}

const VALUES: any[] = [];
for (let k = 0; k <= 51; k++) VALUES.push(mk(k));
VALUES.push(-(Number.MIN_VALUE * Math.pow(2, 28))); // negative denormal
VALUES.push(0);                                     // +0
VALUES.push(-0);                                    // -0
VALUES.push(5);                                     // normal int control
VALUES.push(1.5);                                   // normal double control

// --- property reads on a pointer-shaped number: undefined / Number.prototype
for (const e of VALUES) {
  console.log(e.x, e.size, e[0], e["y"]);
  console.log(typeof e, typeof e === "number");
  console.log(e.constructor === Number, typeof e.toFixed);
  console.log(e?.x, e?.[0]);
}

// --- property stores on a number: TypeError in strict (ESM) code, and the
// value is unchanged. The store must reach the primitive-[[Set]] path, not
// dereference the number's bits.
for (const e of VALUES) {
  const r: any = e;
  try {
    r.x = 5;
    console.log("set.x", r.x);
  } catch (x: any) {
    console.log("set.x THROW", x.name);
  }
  // (An indexed store `r[0] = 5` on a Number does not throw in Perry yet,
  // for any Number: a separate strict-mode gap, not exercised here.)
  console.log(typeof r, r === e);
}

// --- in / instanceof: no deref of the number's bits
for (const e of VALUES) {
  console.log(e instanceof Object, e instanceof Number, e instanceof Array);
}

// --- Map: a denormal key hashes and compares as a number (small and large)
for (const e of VALUES) {
  const m = new Map<any, any>();
  m.set(e, 1);
  console.log(m.get(e), m.has(e), m.size);
}
{
  // a large Map, where numeric keys take the hashed/pointer-key index
  const m = new Map<any, any>();
  for (let i = 0; i < VALUES.length; i++) m.set(VALUES[i], i);
  for (let i = 0; i < VALUES.length; i++) console.log(m.get(VALUES[i]));
  console.log(m.size);
}

// --- Set: a denormal element hashes and compares as a number
for (const e of VALUES) {
  const s = new Set<any>();
  s.add(e);
  s.add(e);
  console.log(s.has(e), s.size);
}

// --- Object.* / Reflect: a number is a non-object target
for (const e of VALUES) {
  console.log(Object.keys(e).length, Object.values(e).length);
  console.log(Object.getPrototypeOf(e) === Number.prototype);
  console.log(Object.prototype.toString.call(e));
  console.log(Object.getOwnPropertyNames(e).length);
  console.log(Object.is(e, e), Object.is(e, 0), Object.is(e, -0));
}

// --- serialization / string coercion read the numeric value
for (const e of VALUES) {
  console.log(JSON.stringify(e), JSON.stringify({ a: e, b: [e] }));
  console.log(String(e), `${e}`, e + "", e.toString());
  console.log(Number.isFinite(e), Number.isInteger(e), isNaN(e));
}

// --- the number survives storage in containers / fields / closures
for (const e of VALUES) {
  const arrD: number[] = [e, 1.5, 2.5];
  const arrM: any[] = [e, "s", {}, null];
  const o: any = { f: e };
  o.z = e;
  const c = () => e;
  console.log(Object.is(arrD[0], e), Object.is(arrM[0], e));
  console.log(Object.is(o.f, e), Object.is(o.z, e), Object.is(c(), e));
}

// --- equality / arithmetic are bit-exact numeric
for (const e of VALUES) {
  console.log(e === e, e == e, e === mk(28) || e !== mk(28), e === 0, e == 0);
  console.log(Object.is(e * 1, e), Object.is(e / 1, e), e + 1);
}

console.log("denormal-receivers-ok");

// --- computed access with a literal / symbol key: the receiver is that
// Number (its own value reaches the method), not a placeholder.
for (const e of VALUES) {
  const r: any = e;
  console.log(r["toFixed"](2), r["toString"]() === String(e), r["valueOf"]() === e || (e !== e));
  console.log(r["constructor"] === Number, r["constructor"].name, r[Symbol.toPrimitive], r[Symbol.iterator]);
  console.log(Object.is(r["valueOf"](), e), r.toExponential(3) === e.toExponential(3));
}

// --- a getter and a `length` the program installs on Number.prototype: the
// method call's [[Get]] and `e.length` read the Number, never an address.
Object.defineProperty(Number.prototype, "den", {
  get(this: any) { "use strict"; return "den:" + String(this); },
  configurable: true,
});
Object.defineProperty(Number.prototype, "length", { value: 7, configurable: true });
for (const e of VALUES) {
  const r: any = e;
  console.log(r.den, r["den"], r.length, r["length"]);
  console.log(r.toFixed(2), r["toString"]() === String(e), r.valueOf() === e || e !== e);
}
delete (Number.prototype as any).den;
delete (Number.prototype as any).length;
