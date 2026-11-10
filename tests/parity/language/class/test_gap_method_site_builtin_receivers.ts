// Indirect calls keep these sites untyped even after argument inlining.
function pushOne(q: any, value: number) { return q.push(value); }
function popOne(q: any) { return q.pop(); }
function getOne(m: any, key: string) { return m.get(key); }
function drive(fn: any, receiver: any, value: any, count: number) {
  let result: any;
  for (let i = 0; i < count; i++) result = fn(receiver, value);
  return result;
}
const q: any = [];
drive(pushOne, q, 7, 100);
console.log("warm", q.length, drive(popOne, q, 0, 100), q.length);
const map = new Map([["x", 42]]);
console.log("map warm", drive(getOne, map, "x", 100));
// Patch through runtime prototype values so the compiler cannot divert the
// whole file around method sites based on a syntactic prototype assignment.
const arrayPrototype: any = Object.getPrototypeOf(q);
const mapPrototype: any = Object.getPrototypeOf(map);
const originalPush = arrayPrototype.push;
arrayPrototype.push = function (...items: any[]) { return 700 + items.length; };
console.log("patched", drive(pushOne, q, 9, 3), q.length);
arrayPrototype.push = originalPush;
console.log("restored", drive(pushOne, q, 8, 3), q.length);
function pushWithArgument(a: any, argument: any) { return a.push(argument()); }
function invokePush(fn: any, a: any, argument: any) { return fn(a, argument); }
console.log("push split warm", invokePush(pushWithArgument, q, () => 10));
console.log("push split replace", invokePush(pushWithArgument, q, () => {
  arrayPrototype.push = function () { return 1111; };
  return 11;
}), q[q.length - 1]);
console.log("push split next", invokePush(pushWithArgument, q, () => 12));
arrayPrototype.push = originalPush;
q.push = function (v: number) { return v + 900; };
console.log("own", drive(pushOne, q, 2, 3), q.length);
delete q.push;
class ArrayChild extends Array {
  push(...values: any[]) { return 800 + values.length; }
}
console.log("subclass", drive(pushOne, new ArrayChild(), 3, 3));
let gets = 0;
const target: any[] = [];
const proxy = new Proxy(target, {
  get(t, key, receiver) { if (key === "push") gets++; return Reflect.get(t, key, receiver); }
});
console.log("proxy", drive(pushOne, proxy, 5, 3), target.join(","), gets);
const frozen: any = Object.freeze([1]);
try { drive(pushOne, frozen, 2, 1); console.log("frozen missed"); }
catch (e) { console.log("frozen", e instanceof TypeError, frozen.length); }
try { drive(popOne, frozen, 0, 1); console.log("frozen pop missed"); }
catch (e) { console.log("frozen pop", e instanceof TypeError, frozen.length); }
for (const items of [[], [2, 3]]) {
  try { Reflect.apply(originalPush, frozen, items); console.log("frozen apply missed"); }
  catch (e: any) { console.log("frozen apply", items.length, e.message); }
}
const holey: any = [1, , 3];
console.log("holey", drive(popOne, holey, 0, 2), holey.length, 1 in holey);
const sparse: any = [];
sparse[10000] = 4;
console.log("sparse", drive(pushOne, sparse, 5, 1), drive(popOne, sparse, 0, 1), sparse.length, 9999 in sparse);
const described: any = [1, 2];
const legacy: any = [1, 2];
legacy.pos = 10;
legacy.end = 20;
console.log("legacy warm", drive(pushOne, legacy, 3, 100), legacy.pos, legacy.end);
legacy.push = function () { return 333; };
console.log("legacy override", drive(pushOne, legacy, 4, 3), legacy.length);
delete legacy.push;
console.log("legacy restored", drive(pushOne, legacy, 5, 3), legacy.length);
let legacyGets = 0;
Object.defineProperty(legacy, "push", {
  get() { legacyGets++; return originalPush; }, configurable: true
});
console.log("legacy getter", drive(pushOne, legacy, 6, 3), legacyGets, legacy.length);
delete legacy.push;
Object.defineProperty(described, "pos", { value: 10, writable: true, enumerable: true, configurable: true });
described.end = 20;
console.log("described warm", drive(pushOne, described, 3, 100), described.pos, described.end);
described.push = function () { return 444; };
console.log("described override", drive(pushOne, described, 4, 3), described.length);
delete described.push;
console.log("described restored", drive(pushOne, described, 5, 3), described.length);
function sliceOne(a: any) { return a.slice(); }
const copied: any = drive(sliceOne, described, 0, 3);
console.log("described slice", copied.length, copied[0], copied[copied.length - 1], copied.pos);
class MapChild extends Map {
  get(key: any) { return 100 + (super.get(key) || 0); }
}
console.log("map subclass", drive(getOne, new MapChild([["x", 4]]), "x", 3));
class PlainMapChild extends Map {}
console.log("map inherited", drive(getOne, new PlainMapChild([["x", 5]]), "x", 3));
const originalGet = mapPrototype.get;
mapPrototype.get = function () { return 600; };
console.log("map patched", drive(getOne, map, "x", 3));
mapPrototype.get = originalGet;
console.log("map restored", drive(getOne, map, "x", 3));
// The method read precedes an argument that replaces it.
function getWithArgument(m: any, argument: any) { return m.get(argument()); }
function invokeGet(fn: any, m: any, argument: any) { return fn(m, argument); }
console.log("split warm", invokeGet(getWithArgument, map, () => "x"));
console.log("split replace", invokeGet(getWithArgument, map, () => {
  mapPrototype.get = function () { return 999; };
  return "x";
}));
console.log("split next", drive(getOne, map, "x", 1));
mapPrototype.get = originalGet;
map.get = function () { return 555; };
console.log("map own", drive(getOne, map, "x", 3));
delete (map as any).get;
const customArray: any = [];
Object.setPrototypeOf(customArray, { push(v: number) { return v + 321; } });
console.log("custom prototype", drive(pushOne, customArray, 4, 3));
function setHas(s: any, value: number) { return s.has(value); }
const set = new Set([2, 4]);
console.log("set warm", drive(setHas, set, 4, 100));
set.has = function () { return false; };
console.log("set own", drive(setHas, set, 4, 3));
// Native argument-buffer bodies preserve omission, explicit undefined,
// borrowed receivers and variadic arguments through the value-call bridge.
function includesOne(a: any, value: any) { return a.includes(value); }
const searched: any = [1, , 3];
console.log("search warm", drive(includesOne, searched, undefined, 100));
const originalIncludes = arrayPrototype.includes;
arrayPrototype.includes = function () { return "changed"; };
console.log("search patched", drive(includesOne, searched, 1, 3));
arrayPrototype.includes = originalIncludes;
console.log("search restored", drive(includesOne, searched, 3, 3));
const like: any = { length: 3, 0: 1, 2: 3 };
const invoke: any = Reflect.apply;
const callback: any = (v: number) => v * 2;
console.log("borrowed map", invoke(arrayPrototype.map, like, [callback]).join(","));
console.log("borrowed filter", invoke(arrayPrototype.filter, like, [callback]).join(","));
console.log("borrowed some every", invoke(arrayPrototype.some, like, [callback]), invoke(arrayPrototype.every, like, [callback]));
console.log("borrowed find", invoke(arrayPrototype.find, like, [(v: any) => v === undefined]), invoke(arrayPrototype.findIndex, like, [(v: any) => v === undefined]));
console.log("borrowed last", invoke(arrayPrototype.findLast, like, [callback]), invoke(arrayPrototype.findLastIndex, like, [callback]));
let visited = 0;
invoke(arrayPrototype.forEach, like, [(v: number) => { visited += v; }]);
console.log("borrowed forEach", visited);
const reducer: any = (a: any, b: any) => a === undefined ? 10 + b : a + b;
console.log("reduce omitted", invoke(arrayPrototype.reduce, [1, 2], [reducer]));
console.log("reduce undefined", invoke(arrayPrototype.reduce, [1, 2], [reducer, undefined]));
console.log("reduce right", invoke(arrayPrototype.reduceRight, [1, 2], [reducer, 4]));
console.log("search omitted", invoke(arrayPrototype.lastIndexOf, [1, 2, 1], [1]));
console.log("search undefined", invoke(arrayPrototype.lastIndexOf, [1, 2, 1], [1, undefined]));
console.log("indexOf hole", invoke(arrayPrototype.indexOf, searched, [undefined]));
const variadic: any = [1, 2];
console.log("unshift", invoke(arrayPrototype.unshift, variadic, [9, 8]), variadic.join(","));
console.log("splice", invoke(arrayPrototype.splice, variadic, [1, 2, 7, 6]).join(","), variadic.join(","));
console.log("concat", invoke(arrayPrototype.concat, variadic, [[4, 5], 6]).join(","));
console.log("fill", invoke(arrayPrototype.fill, variadic, [3, 1, undefined]).join(","));
console.log("copyWithin", invoke(arrayPrototype.copyWithin, variadic, [0, 2, undefined]).join(","));
console.log("lengths", arrayPrototype.push.length, arrayPrototype.splice.length, arrayPrototype.map.length, arrayPrototype.reduce.length);
