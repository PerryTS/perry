import { parse } from "node:querystring";

console.log("=value:", JSON.stringify(parse("=value")));
console.log("=:", JSON.stringify(parse("=")));
console.log("===:", JSON.stringify(parse("===")));
console.log("=a&=b:", JSON.stringify(parse("=a&=b")));
