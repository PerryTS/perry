import { stringify } from "node:querystring";

console.log("multi:", stringify({ a: "1", b: "2" }, ";;", "::"));
console.log("multi array:", stringify({ a: ["1", "2"], b: "3" }, ";;", "::"));
