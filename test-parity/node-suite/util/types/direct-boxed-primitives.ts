import { isBoxedPrimitive, isBooleanObject, isNumberObject, isStringObject } from "node:util/types";

console.log("direct number object:", isNumberObject(new Number(2)));
console.log("direct string object:", isStringObject(new String("y")));
console.log("direct boolean object:", isBooleanObject(new Boolean(true)));
console.log("direct boxed false:", isBoxedPrimitive(false));
console.log("direct boxed number:", isBoxedPrimitive(new Number(2)));
