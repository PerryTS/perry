// process.permission is only exposed when Node's permission model is enabled.
import processModule from "node:process";

console.log("global permission typeof:", typeof process.permission);
console.log("module permission typeof:", typeof processModule.permission);
console.log("global permission key:", Object.keys(process).includes("permission"));
console.log("module permission key:", Object.keys(processModule).includes("permission"));
console.log("module permission descriptor:", Object.getOwnPropertyDescriptor(processModule, "permission") === undefined);
console.log(
  "global permission descriptor:",
  Object.getOwnPropertyDescriptor(process, "permission") === undefined,
);
