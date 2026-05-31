import constantsDefault from "node:constants";
import * as constantsNs from "node:constants";
import process from "node:process";

const builtin = process.getBuiltinModule("constants");

console.log("namespace has default:", Object.keys(constantsNs).includes("default"));
console.log("namespace default identity:", constantsNs.default === constantsDefault);
console.log("builtin default identity:", builtin === constantsDefault);
console.log("default lacks default key:", !Object.keys(constantsDefault).includes("default"));
console.log("default value works:", constantsDefault.F_OK, typeof constantsDefault.F_OK);
console.log("namespace value same:", constantsNs.F_OK === constantsDefault.F_OK);
