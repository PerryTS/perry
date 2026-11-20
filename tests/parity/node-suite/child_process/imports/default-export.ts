import childProcessDefault from "node:child_process";
import * as childProcessNs from "node:child_process";
import process from "node:process";

const builtin = process.getBuiltinModule("child_process");

console.log("namespace has default:", Object.keys(childProcessNs).includes("default"));
console.log("namespace default identity:", childProcessNs.default === childProcessDefault);
console.log("builtin default identity:", builtin === childProcessDefault);
console.log("default equals namespace:", childProcessDefault === childProcessNs);
console.log("default lacks default key:", !Object.keys(childProcessDefault).includes("default"));
console.log("spawn types:", typeof childProcessDefault.spawn, typeof childProcessNs.spawn);
console.log("spawn identity:", childProcessDefault.spawn === childProcessNs.spawn);
console.log(
  "spawnSync types:",
  typeof childProcessDefault.spawnSync,
  typeof childProcessNs.spawnSync,
);
console.log(
  "ChildProcess types:",
  typeof childProcessDefault.ChildProcess,
  typeof childProcessNs.ChildProcess,
);
