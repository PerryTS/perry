import { execFileSync, execSync, spawnSync, spawn, execFile, exec, fork } from "node:child_process";
const operations = [
  () => execFileSync("perry-wasi-nonexistent-command"),
  () => execSync("perry-wasi-nonexistent-command"),
  () => spawnSync("perry-wasi-nonexistent-command"),
  () => spawn("perry-wasi-nonexistent-command"),
  () => execFile("perry-wasi-nonexistent-command"),
  () => exec("perry-wasi-nonexistent-command"),
  () => fork("perry-wasi-nonexistent-module.js"),
];
let rejected = 0;
for (const operation of operations) {
  try { operation(); }
  catch (error: any) {
    if (error.code === "ERR_NOT_SUPPORTED" && String(error.message).includes("not supported on WASI")) rejected++;
  }
}
console.log("unsupported", rejected);
console.log("continued");
