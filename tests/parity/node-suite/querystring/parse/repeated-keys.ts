import { parse } from "node:querystring";

const parsed = parse("a=1&a=2&a=3");
console.log("is array:", Array.isArray(parsed.a));
console.log("joined:", (parsed.a as string[]).join(","));
