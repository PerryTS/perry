// A tsc-emitted CommonJS re-export module (`__createBinding` with
// Object.getOwnPropertyDescriptor, `__exportStar`, getter re-exports,
// Object.fromEntries, Reflect.apply) in a program whose dynamic code names
// the global scope, so every builtin-namespace call is guarded and the
// generic arm calls the builtins as values.
import * as re from "./cjs2_shapes/reexport.cjs";

const g = new Function("return globalThis")();
console.log(g.Object === Object, typeof g.Reflect.apply);
const r: any = (re as any).default ?? re;
console.log(r.VERSION, r.helper("x"), new r.Widget(3).size(), new r.Renamed(4).size());
const C = r.Client;
const c = new C({});
c.connect();
console.log(c.log.join(","));
console.log(JSON.stringify(r.entries), r.applied);
const d = Object.getOwnPropertyDescriptor(r, "helper");
console.log(typeof d, d ? typeof (d.get ?? d.value) : "none");
console.log(Object.keys(r).filter((k) => k !== "default").sort().join(","));
