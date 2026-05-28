import * as util from "node:util";
import { types } from "node:util";
import {
  isAsyncFunction,
  isGeneratorFunction,
  isGeneratorObject,
  isNativeError,
} from "node:util/types";

async function af() {}
function* gf() {}
console.log("async fn:", types.isAsyncFunction(af));
console.log("generator fn:", types.isGeneratorFunction(gf));
console.log("generator object:", types.isGeneratorObject(gf()));
console.log("native error:", types.isNativeError(new Error("x")));
console.log("namespace async fn:", util.types.isAsyncFunction(af));
console.log("direct async fn:", isAsyncFunction(af));
console.log("direct generator fn:", isGeneratorFunction(gf));
console.log("direct generator object:", isGeneratorObject(gf()));
console.log("direct native error:", isNativeError(new Error("y")));
console.log("date:", types.isDate(new Date()));
console.log("regexp:", types.isRegExp(/x/));
