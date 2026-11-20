import { stringify } from "node:querystring";

console.log("null:", JSON.stringify(stringify(null as unknown as Record<string, unknown>)));
console.log("undefined:", JSON.stringify(stringify(undefined as unknown as Record<string, unknown>)));
console.log("string:", JSON.stringify(stringify("a=1" as unknown as Record<string, unknown>)));
console.log("number:", JSON.stringify(stringify(42 as unknown as Record<string, unknown>)));
console.log("bool:", JSON.stringify(stringify(true as unknown as Record<string, unknown>)));
