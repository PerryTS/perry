import { stringify } from "node:querystring";

console.log("custom:", stringify({ a: "1", b: "2" }, ";", ":"));
console.log("custom array:", stringify({ a: ["1", "2"] }, ";", ":"));
