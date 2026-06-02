import { isDeepStrictEqual } from "node:util";

console.log("number equal:", isDeepStrictEqual(new Number(1), new Number(1)));
console.log("number different:", isDeepStrictEqual(new Number(1), new Number(2)));
console.log("string equal:", isDeepStrictEqual(new String("x"), new String("x")));
console.log("string different:", isDeepStrictEqual(new String("x"), new String("y")));
console.log("boolean equal:", isDeepStrictEqual(new Boolean(false), new Boolean(false)));
console.log("boolean different:", isDeepStrictEqual(new Boolean(false), new Boolean(true)));
console.log("boxed vs primitive:", isDeepStrictEqual(new Number(1), 1));
