import { stringify } from "node:querystring";

console.log("null undefined empty:", stringify({ a: null, b: undefined, c: "" }));
console.log("array null undefined:", stringify({ a: [null, undefined, "", "x"] }));
