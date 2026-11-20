import { stringify } from "node:querystring";

console.log("empty:", JSON.stringify(stringify({})));
console.log("empty array val:", JSON.stringify(stringify({ a: [] })));
console.log("nested empty:", JSON.stringify(stringify({ a: {} as unknown as string })));
