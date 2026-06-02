import { inspect } from "node:util";
console.log("object:", inspect({ a: 1, b: "two" }));
console.log("array:", inspect([1, "two"]));
console.log("null:", inspect(null));
console.log("undefined:", inspect(undefined));
