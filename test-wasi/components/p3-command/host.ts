import { readFileSync } from "node:fs";
import { runComponentCommand } from "perry/component";
declare function gc(): void;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
if (process.argv[2] === "compiled") {
  const bytes = readFileSync(process.argv[3]);
  gc();
  const status = runComponentCommand(bytes, {
    fuel: 1000000000,
    wasi: { inheritStdout: true, inheritStderr: true },
  });
  gc();
  process.exit(status);
} else if (process.argv[2] === "stdio") {
  gc();
  const status = runComponentCommand(readFileSync(process.argv[3]), {
    wasi: { inheritStdin: true, inheritStdout: true },
  });
  gc();
  check(status === 0, "stream completion status");
  console.error("PASS P3 command: exact native stdio completed");
} else {
  for (const [file, status] of [["ok", 0], ["err", 1], ["exit", 23], ["init-exit", 255], ["clock", 0], ["closed-stdio", 0]] as const) {
    gc();
    check(runComponentCommand(readFileSync(process.argv[3] + "/" + file + ".wasm")) === status, file);
    gc();
  }
  for (const file of ["bad-init", "wrong-version", "late-trap"]) {
    let rejected = false;
    try { runComponentCommand(readFileSync(process.argv[3] + "/" + file + ".wasm")); }
    catch (_) { rejected = true; }
    check(rejected, file + " is an embedding failure");
  }
  check(runComponentCommand(readFileSync(process.argv[3] + "/ok.wasm")) === 0, "host survives command exit and failure");
  console.log("PASS P3 command: returns, exit, startup validation, versions, real async clock, closed defaults");
}
