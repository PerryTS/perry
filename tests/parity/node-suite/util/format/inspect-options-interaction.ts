import { formatWithOptions } from "node:util";

console.log(formatWithOptions({ colors: false, depth: 1 }, "value:%O", { a: { b: { c: 1 } } }));
console.log(formatWithOptions({ compact: false }, "%o", { a: 1, b: 2 }));
