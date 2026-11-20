// A builtin's instances inherit from the realm's intrinsic prototype.
// Replacing the writable `globalThis.<Name>` binding afterwards must not
// change what an existing instance inherits from or is an instance of, nor
// the constructor it inherits.
export {};
const g: any = globalThis;
const gpo = Object.getPrototypeOf;
const sab: any = typeof SharedArrayBuffer === "function" ? new SharedArrayBuffer(8) : undefined;
function* gen(): any { yield 1; }
const families: [string, () => any][] = [
  ["Error", () => new Error("e")],
  ["TypeError", () => new TypeError("t")],
  ["RangeError", () => new RangeError("r")],
  ["ReferenceError", () => new ReferenceError("r")],
  ["SyntaxError", () => new SyntaxError("s")],
  ["EvalError", () => new EvalError("e")],
  ["URIError", () => new URIError("u")],
  ["AggregateError", () => new AggregateError([], "a")],
  ["Date", () => new Date(0)],
  ["Promise", () => Promise.resolve(1)],
  ["ArrayBuffer", () => new ArrayBuffer(4)],
  ["SharedArrayBuffer", () => sab],
  ["DataView", () => new DataView(new ArrayBuffer(4))],
  ["Set", () => new Set([1])],
  ["Map", () => new Map([[1, 2]])],
  ["WeakMap", () => new WeakMap()],
  ["WeakSet", () => new WeakSet()],
  ["WeakRef", () => new WeakRef({})],
  ["FinalizationRegistry", () => new FinalizationRegistry(() => {})],
  ["RegExp", () => /a/g],
  ["Array", () => [1, 2]],
  ["Object", () => ({ a: 1 })],
  ["Function", () => function f() {}],
  ["Boolean", () => new Boolean(true)],
  ["Number", () => new Number(1)],
  ["String", () => new String("s")],
  ["Symbol", () => Object(Symbol("s"))],
  ["BigInt", () => Object(BigInt(1))],
  ["Int8Array", () => new Int8Array(1)],
  ["Uint8Array", () => new Uint8Array(1)],
  ["Uint8ClampedArray", () => new Uint8ClampedArray(1)],
  ["Int16Array", () => new Int16Array(1)],
  ["Uint16Array", () => new Uint16Array(1)],
  ["Int32Array", () => new Int32Array(1)],
  ["Uint32Array", () => new Uint32Array(1)],
  ["Float32Array", () => new Float32Array(1)],
  ["Float64Array", () => new Float64Array(1)],
  ["Float16Array", () => new Float16Array(1)],
  ["BigInt64Array", () => new BigInt64Array(1)],
  ["BigUint64Array", () => new BigUint64Array(1)],
  ["Iterator", () => [1].values()],
];
const rows: string[] = [];
for (const [name, make] of families) {
  const original = g[name];
  const value = make();
  const proto = gpo(value);
  const replacement: any = function Replacement() {};
  g[name] = replacement;
  // Read everything while the binding is replaced, print after restoring it.
  const instance = value instanceof replacement;
  const viaGlobal = value instanceof g[name];
  const same = gpo(value) === proto;
  const stillOriginal = value instanceof original;
  const ctor = value.constructor === original;
  g[name] = original;
  rows.push(name + " " + instance + " " + viaGlobal + " " + same + " " + stillOriginal + " " + ctor);
}
// %Array.prototype%'s own `constructor` stays %Array%; a promise resolves
// through %Promise%, so `Promise.resolve(p)` still returns `p` itself.
{
  const A = g.Array, P = g.Promise;
  const p = P.resolve(1);
  g.Array = function Replacement() {};
  g.Promise = function Replacement() {};
  const own = Object.getOwnPropertyDescriptor(A.prototype, "constructor");
  const arrayCtor = own !== undefined && own.value === A;
  const same = P.resolve(p) === p;
  g.Array = A;
  g.Promise = P;
  rows.push("array-own-ctor " + arrayCtor + " promise-resolve-same " + same);
}
// Generators: %GeneratorPrototype% is no global; replacing Function must not move it.
const it = gen();
const genProto = gpo(it);
const F = g.Function;
g.Function = function Replacement() {};
const genSame = gpo(gen()) === genProto && gpo(it) === genProto;
g.Function = F;
rows.push("generator " + genSame);
for (const row of rows) console.log(row);
