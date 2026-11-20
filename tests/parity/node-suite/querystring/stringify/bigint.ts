import { stringify } from "node:querystring";

console.log("small:", stringify({ a: 1n as unknown as string }));
console.log("large:", stringify({ a: 9007199254740993n as unknown as string }));
console.log("negative:", stringify({ a: (-42n) as unknown as string }));
console.log("array:", stringify({ a: [1n, 2n, 3n] as unknown as string[] }));
