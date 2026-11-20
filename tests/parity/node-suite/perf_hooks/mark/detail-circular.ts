import { performance } from "node:perf_hooks";
// mark detail with a circular reference is accepted (structuredClone handles
// cycles via reference preservation).
const o: any = {};
o.self = o;
try {
  const mark = performance.mark("c", { detail: o });
  console.log("distinct clone:", mark.detail !== o);
  console.log("cycle preserved:", mark.detail.self === mark.detail);
} finally {
  performance.clearMarks();
}
