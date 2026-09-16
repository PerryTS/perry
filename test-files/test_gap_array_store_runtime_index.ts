// Array element stores whose numeric key carries no static range proof: the
// runtime-canonical element tier must agree with the exact property-key path.

function put(a: number[], k: number, v: number): void {
  a[k] = v;
}
function putAny(a: any[], k: number, v: any): void {
  a[k] = v;
}
function show(label: string, a: unknown[]) {
  const isIndexKey = (k: string) =>
    k.length > 0 && k.split("").every((c) => c >= "0" && c <= "9");
  const named = Object.keys(a).filter((k) => !isIndexKey(k));
  console.log(label, JSON.stringify(a), a.length, named.join("|"));
}

const keys: number[] = [0, 1, 2.5, -1, -0, NaN, Infinity, 2 ** 32 - 1, 4, 3];
const arr: number[] = [10, 20, 30, 40];
for (const k of keys) {
  put(arr, k, k === k ? k * 2 : -7);
}
show("mixed keys", arr);
console.log("named props", (arr as any)["2.5"], (arr as any)["-1"], (arr as any)["NaN"], (arr as any)["Infinity"], (arr as any)["4294967295"]);
const big: number[] = [1];
put(big, 2 ** 31, 5);
console.log("large index", big.length, big[2 ** 31], Object.keys(big).length);

// Hot in-bounds stores, then an extend and a sparse store.
const hot: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
for (let i = 0; i < 5000; i++) {
  put(hot, (i * 7) % 8, i);
}
put(hot, hot.length, 99);
put(hot, 12, 100);
show("hot", hot);

// Int32-boxed keys from a Set.
const boxedKeys = new Set<number>();
for (let i = 0; i < 4; i++) boxedKeys.add(i | 0);
const target: number[] = [0, 0, 0, 0];
for (const k of boxedKeys) put(target, k, k + 0.5);
show("set keys", target);

// Non-number values into a numeric array and a mixed array.
const mixed: any[] = [1, 2, 3];
putAny(mixed, 1, "two");
putAny(mixed, 2, { three: 3 });
putAny(mixed, 0, null);
show("mixed values", mixed);

// Frozen / sealed arrays reject the store (module code is strict).
function tryPut(label: string, a: number[], k: number, v: number) {
  try {
    put(a, k, v);
    console.log(label, "stored");
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}
const frozen: number[] = Object.freeze([1, 2, 3]) as number[];
tryPut("frozen in-bounds", frozen, 1, 50);
show("frozen", frozen);
const sealed: number[] = Object.seal([1, 2, 3]);
tryPut("sealed in-bounds", sealed, 1, 50);
tryPut("sealed extend", sealed, 3, 60);
show("sealed", sealed);

// An accessor defined on an index intercepts the store.
const withSetter: number[] = [1, 2, 3];
Object.defineProperty(withSetter, 1, {
  set(v: number) {
    console.log("setter saw", v);
  },
  get() {
    return -1;
  },
});
put(withSetter, 1, 42);
console.log("accessor", withSetter[1]);

// Array.prototype index setter (prototype pollution) sees holes.
const holey: number[] = [1, , 3] as number[];
Object.defineProperty(Array.prototype, 1, {
  set(v: number) {
    console.log("proto setter saw", v);
  },
  get() {
    return undefined;
  },
  configurable: true,
});
put(holey, 1, 7);
console.log("proto setter", JSON.stringify(holey), Object.prototype.hasOwnProperty.call(holey, 1));
delete (Array.prototype as any)[1];

// Objects allocated during the store expression survive a collection.
const objs: any[] = new Array(64).fill(null);
for (let i = 0; i < 20000; i++) {
  putAny(objs, (i * 13) % 64, { i, s: "v" + i });
}
console.log("objects", objs[0].s, objs[63].s, objs.length);
