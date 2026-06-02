import { stringify } from "node:querystring";

console.log("single space:", stringify({ a: "hello world" }));
console.log("multi spaces:", stringify({ key: "a  b   c" }));
console.log("plus literal:", stringify({ a: "a+b" }));
console.log("mix:", stringify({ a: "a b+c d" }));
