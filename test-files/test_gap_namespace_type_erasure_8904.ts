// #8904: Zod exposes `z` through `import * as z; export { z }`, and `z.coerce`
// through `export * as coerce`. Both namespace objects must contain all of
// their runtime values and none of their erased TypeScript declarations.
import { z } from "./issue_8904_namespace_materialization/index.ts";

console.log("typeof z:", typeof z);
console.log("typeof z.object:", typeof z.object);
console.log("typeof z.coerce:", typeof z.coerce);
console.log("z keys:", JSON.stringify(Object.keys(z).sort()));
console.log("coerce in z:", "coerce" in z);
console.log(
  "coerce enumerable:",
  Object.getOwnPropertyDescriptor(z, "coerce")?.enumerable,
);
console.log("coerce keys:", JSON.stringify(Object.keys(z.coerce).sort()));
console.log("coerced number:", z.coerce.number()("42"));
