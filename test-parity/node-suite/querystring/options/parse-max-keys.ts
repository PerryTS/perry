import { parse } from "node:querystring";

console.log("max 2:", JSON.stringify(parse("a=1&b=2&c=3", undefined, undefined, { maxKeys: 2 })));
console.log("max 0:", JSON.stringify(parse("a=1&b=2&c=3", undefined, undefined, { maxKeys: 0 })));
console.log("max negative:", JSON.stringify(parse("a=1&b=2&c=3", undefined, undefined, { maxKeys: -1 })));
console.log("max infinity:", JSON.stringify(parse("a=1&b=2&c=3", undefined, undefined, { maxKeys: Infinity })));
console.log("max nan:", JSON.stringify(parse("a=1&b=2&c=3", undefined, undefined, { maxKeys: NaN })));
