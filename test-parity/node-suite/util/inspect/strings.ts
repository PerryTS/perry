import { inspect } from "node:util";

console.log("string:", inspect("hello"));
console.log("escaped:", inspect("a\nb"));
