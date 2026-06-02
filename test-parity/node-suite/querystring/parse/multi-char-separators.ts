import { parse } from "node:querystring";

const parsed = parse("a::1;;b::2;;a::3", ";;", "::");
console.log("a array:", Array.isArray(parsed.a));
console.log("a:", (parsed.a as string[]).join(","));
console.log("b:", parsed.b);
