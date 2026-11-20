import { stringify } from "node:querystring";

console.log("basic:", stringify({ a: "1", b: "2" }));
console.log("number bool:", stringify({ n: 42, ok: true }));
console.log("empty obj:", stringify({}));
