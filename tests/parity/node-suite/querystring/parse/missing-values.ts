import { parse } from "node:querystring";

const parsed = parse("a&b=&=emptykey&c=3");
console.log("a:", JSON.stringify(parsed.a));
console.log("b:", JSON.stringify(parsed.b));
console.log("empty key:", JSON.stringify(parsed[""]));
console.log("c:", parsed.c);
