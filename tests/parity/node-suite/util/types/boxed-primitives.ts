import * as util from "node:util";
const { types } = util;

const n = new Number(1);
const s = new String("x");
const b = new Boolean(false);

console.log("number object:", types.isNumberObject(n));
console.log("number primitive:", types.isNumberObject(1));
console.log("string object:", types.isStringObject(s));
console.log("string primitive:", types.isStringObject("x"));
console.log("boolean object:", types.isBooleanObject(b));
console.log("boolean primitive:", types.isBooleanObject(false));
console.log("boxed n:", types.isBoxedPrimitive(n));
console.log("boxed s:", types.isBoxedPrimitive(s));
console.log("boxed b:", types.isBoxedPrimitive(b));
console.log("boxed plain:", types.isBoxedPrimitive({}));
