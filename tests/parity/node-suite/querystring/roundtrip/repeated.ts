import { parse, stringify } from "node:querystring";

const encoded = stringify({ a: ["1", "2"], b: "x" });
const parsed = parse(encoded);
console.log("encoded:", encoded);
console.log("a array:", Array.isArray(parsed.a));
console.log("a:", (parsed.a as string[]).join(","));
console.log("b:", parsed.b);
