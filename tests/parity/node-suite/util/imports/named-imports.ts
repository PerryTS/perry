import { format, inspect } from "node:util";
console.log("format:", format("%s=%d", "x", 1));
console.log("inspect:", inspect({ a: 1 }));
