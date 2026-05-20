import { parse, stringify } from "node:querystring";

const input = { a: "1", b: "hello world", café: "olé" };
const encoded = stringify(input);
const parsed = parse(encoded);
console.log("encoded:", encoded);
console.log("a:", parsed.a);
console.log("b:", parsed.b);
console.log("café:", parsed.café);
