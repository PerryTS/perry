import { formatWithOptions } from "node:util";
console.log("basic:", formatWithOptions({ colors: false }, "%s=%d", "k", 7));
console.log("object:", formatWithOptions({ colors: false }, "%o", { a: 1 }));
