import { parse } from "node:querystring";

const parsed = parse("a=1&b=2");
console.log("a:", parsed.a);
console.log("b:", parsed.b);
console.log("keys:", Object.keys(parsed).join(","));
