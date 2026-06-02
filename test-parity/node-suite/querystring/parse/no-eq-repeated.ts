import { parse } from "node:querystring";

const parsed = parse("a&a=2&b");
console.log("a array:", Array.isArray(parsed.a));
console.log("a:", (parsed.a as string[]).join(","));
console.log("b:", JSON.stringify(parsed.b));
