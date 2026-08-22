import { util } from "./fixtures/parity_5891/namespace_const/util_mod.ts";
if (util.fn() !== "FN" || util.arrow() !== "ARROW" || util.num !== 42) throw new Error("namespace values");
if (JSON.stringify(util.objectKeys({ a: 1, b: 2 })) !== '["a","b"]') throw new Error("objectKeys");
const objectKeys = util.objectKeys;
if (JSON.stringify(objectKeys({ x: 1 })) !== '["x"]') throw new Error("local objectKeys");
console.log("OK");
