import { types } from "node:util";
import { isNativeError } from "node:util/types";

class CustomError extends Error {}

console.log("error:", types.isNativeError(new Error("x")));
console.log("typeerror:", types.isNativeError(new TypeError("x")));
console.log("subclass:", types.isNativeError(new CustomError("x")));
console.log("plain:", types.isNativeError({ name: "Error", message: "x" }));
console.log("direct:", isNativeError(new Error("direct")));
