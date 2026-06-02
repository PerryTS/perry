import querystring from "node:querystring";

const s = querystring.stringify({ a: 1, b: 2 }, "&&", "=>");
console.log("string:", s);
const parsed = querystring.parse(s, "&&", "=>");
console.log("parsed:", JSON.stringify(parsed));
