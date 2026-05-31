// #2820 / #2846: Object.getPrototypeOf / setPrototypeOf observable semantics
// and Proxy construction validation + Proxy.revocable record shape.
//
// Compared byte-for-byte against `node --experimental-strip-types`.
//
// Scope note: `Object.getPrototypeOf(1) === Number.prototype` is NOT asserted
// here — Perry has no intrinsic `Number.prototype`/`Object.prototype` singleton
// value to compare against by identity, so the "default" prototype of a plain
// object is asserted via inherited-method presence rather than `=== Object.
// prototype`. The tractable parts implemented and asserted below are:
//   - getPrototypeOf(null|undefined) throws TypeError
//   - getPrototypeOf(Object.create(null)) === null
//   - default object's prototype is non-null (distinct from create(null))
//   - setPrototypeOf returns the target, mutates the observable [[Prototype]],
//     and makes inherited reads resolve
//   - setPrototypeOf(null, ...) throws; setPrototypeOf(obj, 1) throws
//   - setPrototypeOf(obj, null) makes getPrototypeOf return null
//   - new Proxy(non-object, ...) throws TypeError (both positions)
//   - Proxy.revocable returns { proxy, revoke }, forwards before revoke,
//     and throws on access after revoke (idempotent revoke)

// --- getPrototypeOf nullish throws ---
let threw = false;
try {
  Object.getPrototypeOf(null as any);
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("getProto(null) throws TypeError:", threw);

threw = false;
try {
  Object.getPrototypeOf(undefined as any);
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("getProto(undefined) throws TypeError:", threw);

// --- Object.create(null) has null prototype ---
const bareCreate: any = Object.create(null);
console.log("getProto(create(null)) === null:", Object.getPrototypeOf(bareCreate) === null);

// --- default object has a non-null prototype (distinct from create(null)) ---
const plain: any = { keep: 1 };
const plainProto = Object.getPrototypeOf(plain);
console.log("default proto non-null:", plainProto !== null && plainProto !== undefined);

// --- setPrototypeOf returns target, mutates [[Prototype]], inherited read ---
const proto: any = { x: 1 };
const obj: any = {};
const ret = Object.setPrototypeOf(obj, proto);
console.log("setProto returns target:", ret === obj);
console.log("getProto reflects set proto:", Object.getPrototypeOf(obj) === proto);
console.log("inherited read obj.x:", obj.x);

// --- setPrototypeOf validation throws ---
threw = false;
try {
  Object.setPrototypeOf(null as any, proto);
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("setProto(null, proto) throws:", threw);

threw = false;
try {
  Object.setPrototypeOf({}, 1 as any);
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("setProto(obj, 1) throws:", threw);

// --- setPrototypeOf(obj, null) ---
const bare: any = {};
Object.setPrototypeOf(bare, null);
console.log("setProto(obj, null) -> getProto null:", Object.getPrototypeOf(bare) === null);

// --- Proxy construction validation ---
threw = false;
try {
  new Proxy(5 as any, {});
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("new Proxy(5, {}) throws TypeError:", threw);

threw = false;
try {
  new Proxy({} as any, 1 as any);
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("new Proxy({}, 1) throws TypeError:", threw);

// --- Proxy.revocable record ---
const rec: any = Proxy.revocable({ a: 1 }, {});
console.log("typeof rec.proxy:", typeof rec.proxy);
console.log("typeof rec.revoke:", typeof rec.revoke);
console.log("rec.proxy.a before revoke:", rec.proxy.a);

// revoke via stored alias, not destructuring
const r = rec.revoke;
r();
r(); // idempotent

threw = false;
try {
  const _ = rec.proxy.a;
} catch (e) {
  threw = e instanceof TypeError;
}
console.log("rec.proxy.a after revoke throws:", threw);
