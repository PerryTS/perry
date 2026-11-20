// #11193: every built-in constructor that the spec gives a
// `get [Symbol.species]` accessor must carry it, returning `this`. Perry
// answered `undefined` for all of them. `%TypedArray%` owns the accessor and
// `Uint8Array` & co. inherit it. The species-aware consumers (ArraySpeciesCreate,
// TypedArraySpeciesCreate, RegExp split/matchAll, Promise then/finally,
// ArrayBuffer slice) must keep their default results now that the lookup
// finds the intrinsic instead of `undefined`.

const S = Symbol.species;
const ctors: [string, any][] = [
  ["Array", Array],
  ["Map", Map],
  ["Set", Set],
  ["Promise", Promise],
  ["RegExp", RegExp],
  ["ArrayBuffer", ArrayBuffer],
  ["SharedArrayBuffer", SharedArrayBuffer],
];
for (const [n, C] of ctors) {
  const d = Object.getOwnPropertyDescriptor(C, S)!;
  console.log(
    n,
    "get:", typeof d.get,
    "set:", typeof d.set,
    "enumerable:", d.enumerable,
    "configurable:", d.configurable,
    "name:", d.get!.name,
    "length:", d.get!.length,
    "value is ctor:", C[S] === C,
  );
}

const TA = Object.getPrototypeOf(Uint8Array);
const td = Object.getOwnPropertyDescriptor(TA, S)!;
console.log("TypedArray get:", typeof td.get, "value is ctor:", TA[S] === TA);
for (const [n, C] of [["Uint8Array", Uint8Array], ["Int32Array", Int32Array], ["Float64Array", Float64Array]] as [string, any][]) {
  console.log(n, "own:", Object.getOwnPropertyDescriptor(C, S) === undefined ? "none" : "own", "value is ctor:", C[S] === C);
}

// The getter returns its receiver, whatever it is.
const getter = Object.getOwnPropertyDescriptor(Array, S)!.get!;
const o = {};
console.log("getter this:", getter.call(o) === o, getter.call(Map) === Map);
console.log("distinct getters:", getter !== Object.getOwnPropertyDescriptor(Map, S)!.get);

// Default species results are unchanged.
const doubled = [1, 2, 3].map((x) => x * 2);
console.log(doubled, Array.isArray(doubled), doubled.constructor === Array);
console.log([1, 2, 3].filter((x) => x > 1), [1, 2].concat([3]), [1, 2, 3].slice(1), [1, 2, 3].splice(0, 1));
console.log([[1], [2]].flatMap((x) => x), [1, [2, [3]]].flat(2));
const u8 = new Uint8Array([1, 2, 3]);
const m = u8.map((x) => x * 2);
console.log(m, m.constructor === Uint8Array, u8.slice(1), u8.subarray(1), u8.filter((x) => x !== 2));
console.log(new Float64Array([1.5, 2.5]).slice(1), new Int32Array([4, 5, 6]).subarray(1, 2));
console.log(new ArrayBuffer(8).slice(2).byteLength, new SharedArrayBuffer(8).slice(3).byteLength);
console.log(Buffer.from("hello").slice(1).toString(), Buffer.isBuffer(Buffer.from("abc").subarray(1)));
console.log("a,b,c".split(/,/), "x1y2z".split(/\d/, 2), [..."a1b22".matchAll(/\d+/g)].map((r) => r[0]));
console.log("a-b".replace(/-/g, "+"), /b/y.test("ab"));

// A user species still wins over the intrinsic.
const arr: any = [1, 2, 3];
arr.constructor = { [S]: function Custom(this: any, n: number) { this.tag = "custom"; this.length = n; } };
const custom = arr.map((x: number) => x);
console.log("user species:", custom.tag, Array.isArray(custom));
const nullSpecies: any = [1, 2];
nullSpecies.constructor = { [S]: null };
console.log("null species:", Array.isArray(nullSpecies.map((x: number) => x)));

Promise.resolve(1)
  .then((v) => {
    console.log("then", v);
    return v + 1;
  })
  .finally(() => console.log("finally"))
  .then((v) => console.log("after finally", v));
Promise.reject(new Error("boom")).catch((e) => console.log("catch", e.message));
