import { parse } from "node:url";

const a = parse("https://example.com/p?a=1&b=2");
console.log("default query type:", typeof a.query);
console.log("default query:", a.query);

const b = parse("https://example.com/p?a=1&b=2", false);
console.log("false query type:", typeof b.query);
console.log("false query:", b.query);

const c = parse("https://example.com/p?a=1&b=2", true);
console.log("true query type:", typeof c.query);
console.log("true query a:", (c.query as Record<string, string>).a);
