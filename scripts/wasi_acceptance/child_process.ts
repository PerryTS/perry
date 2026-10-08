import { execFileSync } from "node:child_process";
try {
  execFileSync("perry-wasi-nonexistent-command");
} catch (error) {
  console.log(String(error.message).includes("not supported on WASI"));
}
console.log("continued");
