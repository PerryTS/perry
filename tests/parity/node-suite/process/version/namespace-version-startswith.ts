import * as process from "node:process";
import * as proc from "node:process";

console.log("namespace version typeof:", typeof process.version);
console.log("namespace version starts v:", process.version.startsWith("v"));
console.log("namespace alias starts v:", proc.version.startsWith("v"));
