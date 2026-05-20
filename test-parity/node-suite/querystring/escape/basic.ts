import { escape } from "node:querystring";

console.log("ascii:", escape("hello"));
console.log("space:", escape("hello world"));
console.log("reserved:", escape("a&b=c"));
console.log("symbols:", escape("!*'()~"));
