import * as sysNs from "node:sys";
import * as utilNs from "node:util";
import sysDefault from "node:sys";
import utilDefault from "node:util";

const sys = sysDefault;
const util = utilDefault;

console.log("default identity:", sys === util);
console.log(
  "namespace default identity:",
  sysNs.default === utilNs.default,
  sysNs.default === sys,
  utilNs.default === util,
);

for (const name of [
  "format",
  "inspect",
  "debuglog",
  "isArray",
  "types",
  "TextEncoder",
  "TextDecoder",
  "parseArgs",
  "stripVTControlCharacters",
]) {
  console.log(name + ":", typeof sys[name], sys[name] === util[name]);
}

console.log(
  "types predicates:",
  typeof sys.types.isArrayBuffer,
  sys.types.isArrayBuffer === util.types.isArrayBuffer,
);
console.log(
  "namespace member identity:",
  sysNs.format === utilNs.format,
  sysNs.types === utilNs.types,
);
console.log("keys:", Object.keys(sys).length === Object.keys(util).length);
