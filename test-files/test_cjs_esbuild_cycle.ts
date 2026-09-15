// #10178: an ESM import enters an esbuild CommonJS require cycle.
import process from "node:process";
import token from "./cjs_esbuild_cycle/token.cjs";

if (typeof process.emitWarning !== "function") throw new Error("missing warning support");
console.log(token.cycleView());
console.log(token.reads());
delete globalThis.__esbuildCycleRecord;
console.log(token.callThroughCycle());
token.update();
console.log(token.callThroughCycle());
console.log(token.reads());
