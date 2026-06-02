import { performance } from "node:perf_hooks";
// mark detail with a circular reference is accepted (structuredClone handles
// cycles via reference preservation).
const o: any = {};
o.self = o;
let ok = false;
try {
  performance.mark("c", { detail: o });
  ok = true;
} catch {
  ok = false;
}
console.log("accepted:", ok);
