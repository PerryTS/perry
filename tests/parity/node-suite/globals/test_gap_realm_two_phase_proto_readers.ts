// The runtime's readers of the realm's intrinsic prototypes, made hot:
// `Object.getPrototypeOf` of values of many kinds, prototype-chain walks,
// `instanceof` against builtins and inherited method values read as values.
// A large program does this 10^5 times while it starts; the two-phase realm
// must not make one of these reads cost more than it did (the measured case
// of PR-realm2-2.md, "the Supabase gate").
class Base { tag() { return "b"; } }
class Derived extends Base { tag() { return "d"; } }
const samples: any[] = [
  [], [1, 2], {}, { a: 1 }, () => 1, function named() {}, new Map(), new Set(), new Date(0), /x/g,
  Promise.resolve(1), new Base(), new Derived(), new Error("e"), new TypeError("t"), new Uint8Array(2),
  "str", 1, true, Symbol("s"), 10n, Base, Derived, Object.create(null), Object.create({ inherited: 1 }),
];
function depth(value: any): number {
  let n = 0;
  let p = value === null || value === undefined ? null : Object.getPrototypeOf(value);
  while (p !== null && n < 16) { n++; p = Object.getPrototypeOf(p); }
  return n;
}
let depths = 0, arrays = 0, objects = 0, functions = 0, methods = 0, instances = 0;
const rounds = 2000;
for (let i = 0; i < rounds; i++) {
  for (const s of samples) {
    depths += depth(s);
    const p = s === null ? null : Object.getPrototypeOf(s);
    if (p === Array.prototype) arrays++;
    if (p === Object.prototype) objects++;
    if (p === Function.prototype) functions++;
    if (s instanceof Object) instances++;
    if (s instanceof Error) instances++;
    const push = (s as any).push;
    if (push === Array.prototype.push) methods++;
  }
}
console.log("proto readers", rounds, depths, arrays, objects, functions, methods, instances);
