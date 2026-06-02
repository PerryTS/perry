import { unescape } from "node:querystring";

console.log("space:", unescape("hello%20world"));
console.log("reserved:", unescape("a%26b%3Dc"));
console.log("plus:", unescape("a+b+c"));
console.log("invalid percent:", unescape("abc%zzdef"));
