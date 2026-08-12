// #7189: `export * as ns from "./m.ts"` must appear on the re-exporting
// module's NAMESPACE OBJECT, not only as a named binding. Before the fix
// `B.deep` was `undefined` and `Object.keys(B)` omitted it entirely, while
// `import { deep }` of the same thing worked — which is the shape that made
// zod's `z.coerce` / `z.iso` / `z.core` / `z.locales` all undefined.
import * as B from "./issue_7189_namespace_object_reexport/mid.ts";
import * as C from "./issue_7189_namespace_object_reexport/outer.ts";
import { deep } from "./issue_7189_namespace_object_reexport/mid.ts";

console.log("typeof B.deep      :", typeof B.deep);
console.log("keys(B)            :", JSON.stringify(Object.keys(B).sort()));
console.log("B.gamma            :", B.gamma);

// The named import of the same binding always worked; it must keep working.
console.log("typeof deep        :", typeof deep, deep.alpha);

// Members reached THROUGH the namespace object.
console.log("B.deep.alpha       :", B.deep.alpha);
console.log("B.deep.beta()      :", B.deep.beta());
console.log("keys(B.deep)       :", JSON.stringify(Object.keys(B.deep).sort()));

// Two levels: a namespace re-export whose target itself re-exports one.
console.log("C.inner.gamma      :", C.inner.gamma);
console.log("C.inner.deep.alpha :", C.inner.deep.alpha);
console.log("keys(C)            :", JSON.stringify(Object.keys(C).sort()));
console.log("C.delta            :", C.delta);
