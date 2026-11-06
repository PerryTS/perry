import { types } from "node:util";

async function af() {}
function* gf() {}
console.log("async fn:", types.isAsyncFunction(af));
console.log("generator fn:", types.isGeneratorFunction(gf));
console.log("generator object:", types.isGeneratorObject(gf()));
console.log("native error:", types.isNativeError(new Error("x")));
console.log("date:", types.isDate(new Date()));
console.log("regexp:", types.isRegExp(/x/));
