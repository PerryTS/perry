import { createInterface, Interface, Readline } from "node:readline/promises";
import * as readlinePromises from "node:readline/promises";

const builtin = process.getBuiltinModule("readline/promises") as any;
const prefixed = process.getBuiltinModule("node:readline/promises") as any;

console.log("named createInterface:", typeof createInterface);
console.log("named Interface:", typeof Interface);
console.log("named Readline:", typeof Readline);
console.log("namespace createInterface:", typeof readlinePromises.createInterface);
console.log("builtin type:", typeof builtin);
console.log("builtin createInterface:", typeof builtin.createInterface);
console.log("builtin Interface:", typeof builtin.Interface);
console.log("builtin Readline:", typeof builtin.Readline);
console.log("prefix identity:", builtin === prefixed);
console.log("builtin keys:", Object.keys(builtin).sort().join(","));
console.log("unknown:", process.getBuiltinModule("readline/not-real"));
