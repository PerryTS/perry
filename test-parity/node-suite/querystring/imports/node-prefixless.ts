import { parse, stringify } from "querystring";

console.log("parse:", JSON.stringify(parse("a=1&b=2")));
console.log("stringify:", stringify({ a: "1", b: "2" }));
