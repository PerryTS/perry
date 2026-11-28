// ENUM-DEL: the bindings the realm installs on the global object are
// non-enumerable ({[[Enumerable]]: false}, ECMA-262 17, 19-28), so for-in over
// globalThis (14.7.5.9 EnumerateObjectProperties) never yields them, and
// `for (w in s) delete s[w]` over a map that holds globalThis removes only the
// keys for-in yields: `Object`, `Array`, ... stay; a program global goes.
const realm = [
  "Object", "Array", "Function", "Float32Array", "Int8Array", "Map", "Set", "Promise",
  "Symbol", "Math", "JSON", "Reflect", "Error", "TypeError", "console", "process",
  "Proxy", "parseInt", "NaN", "undefined", "Infinity",
];
const seen: string[] = [];
for (const k in globalThis) seen.push(k);
let enumerated = 0;
for (const n of realm) {
  if (seen.indexOf(n) >= 0) {
    console.log("enumerated", n);
    enumerated++;
  }
}
console.log("realm bindings enumerated:", enumerated);
(globalThis as any).programGlobal = 1;
const maps: Record<string, any> = { a: { x: 1, y: 2 }, b: globalThis };
for (const id in maps) {
  const s = maps[id];
  for (const w in s) delete s[w];
}
console.log(Object.keys(maps.a).length);
console.log(typeof Object, typeof Array, typeof Float32Array, typeof Math, typeof JSON, typeof Map);
console.log(typeof (globalThis as any).programGlobal);
console.log(Object.getPrototypeOf({}) === Object.prototype, new Float32Array(2).length);
