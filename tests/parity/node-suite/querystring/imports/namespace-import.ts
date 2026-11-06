import * as querystring from "node:querystring";

console.log("escape:", querystring.escape("hello world"));
console.log("parse:", JSON.stringify(querystring.parse("a=1")));
console.log("stringify:", querystring.stringify({ a: "1" }));
