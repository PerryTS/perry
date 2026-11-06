import { isDeepStrictEqual } from "node:util";

console.log("number:", isDeepStrictEqual(1, 1));
console.log("number false:", isDeepStrictEqual(1, 2));
console.log("string:", isDeepStrictEqual("x", "x"));
console.log("strict type:", isDeepStrictEqual(1, "1"));
console.log("nan:", isDeepStrictEqual(NaN, NaN));
