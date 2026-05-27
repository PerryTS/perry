import { spawnSync } from "node:child_process";

function text(value: unknown) {
  return value === null ? "null" : String(value);
}

function outputText(output: unknown) {
  return Array.isArray(output) ? output.map(text).join("|") : text(output);
}

const result = spawnSync("sh", ["-c", "printf out; printf err >&2; exit 5"]);
console.log("keys:", Object.keys(result).join(","));
console.log("pid type:", typeof result.pid);
console.log("status:", result.status);
console.log("signal:", result.signal);
console.log("stdout:", text(result.stdout));
console.log("stderr:", text(result.stderr));
console.log("output:", outputText(result.output));
console.log("has error:", Object.prototype.hasOwnProperty.call(result, "error"));

const missing = spawnSync("__perry_missing_command__");
console.log("missing keys:", Object.keys(missing).join(","));
console.log("missing error:", missing.error instanceof Error);
console.log("missing error code:", missing.error && missing.error.code);
console.log("missing status:", missing.status);
console.log("missing signal:", missing.signal);
console.log("missing pid type:", typeof missing.pid);
console.log("missing stdout:", text(missing.stdout));
console.log("missing stderr:", text(missing.stderr));
console.log("missing output:", outputText(missing.output));
