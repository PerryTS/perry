import { formatWithOptions } from "node:util";

console.log(formatWithOptions({ colors: true }, "%s", "x").includes("x"));
console.log(formatWithOptions({ depth: 0 }, "%O", { a: { b: 1 } }));
console.log(formatWithOptions({ sorted: true }, "%O", { b: 2, a: 1 }));
