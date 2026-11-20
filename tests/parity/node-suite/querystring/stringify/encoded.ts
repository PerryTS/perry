import { stringify } from "node:querystring";

console.log("space:", stringify({ "a b": "c d" }));
console.log("utf8:", stringify({ café: "olé" }));
console.log("reserved:", stringify({ "a&b": "c=d" }));
