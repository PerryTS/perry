// Builtin global identifiers (`Object`, `Array`, `Map` ...) are static read
// sites of the global object: a reassignment or redefinition of the global is
// seen by the very next read, and a deleted global reads as such.
const mono: any[] = [{ a: 1 }, [1], new Date(0), "s"];
function check(): string {
  return [
    mono[0].constructor === Object,
    Array.isArray(mono[1]),
    mono[2] instanceof Date,
    typeof Object,
    typeof Map,
    String(mono[3]),
  ].join(",");
}
const out: string[] = [];
for (let i = 0; i < 3; i++) out.push(check());
// reassignment of a global builtin is seen by the identifier
const OrigObject = Object;
const fake: any = function Fake() {};
(globalThis as any).Object = fake;
out.push(String(Object === fake) + "," + String(typeof Object));
(globalThis as any).Object = OrigObject;
out.push(String(Object === OrigObject));
// a global redefined as a data property is read through the same site
const OrigMap = Map;
const fakeMap: any = function FakeMap() {};
Object.defineProperty(globalThis, "Map", { value: fakeMap, writable: true, configurable: true });
out.push(String(Map === fakeMap));
Object.defineProperty(globalThis, "Map", { value: OrigMap, writable: true, configurable: true });
out.push(String(Map === OrigMap) + "," + new Map([[1, 2]]).get(1));
// many reads in a loop under GC pressure
let s = 0;
for (let i = 0; i < 50000; i++) { const o = { v: [i] }; if (o.constructor === Object && Array.isArray(o.v)) s += o.v[0]; }
out.push("sum " + s);
// deleting a global then restoring it
const OrigSet = Set;
delete (globalThis as any).Set;
out.push(typeof (globalThis as any).Set);
(globalThis as any).Set = OrigSet;
out.push(String(new Set([1, 2]).size));
console.log(out.join("\n"));
