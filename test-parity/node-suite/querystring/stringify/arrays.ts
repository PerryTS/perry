import { stringify } from "node:querystring";

console.log("array:", stringify({ a: ["1", "2", "3"] }));
console.log("mixed:", stringify({ a: ["1", "2"], b: "x" }));
