// #1935/#1936/#1937/#1938 — child_process exec/sync error shape, spawnSync
// result fields, encoding option, and execSync throw-on-non-zero. Byte-for-byte
// vs `node --experimental-strip-types`. Avoids printing pid values (per-run).
import * as cp from "node:child_process";

// #1935 — exec failure callback Error carries code/signal/killed/cmd.
await new Promise<void>((resolve) => {
  cp.exec("exit 3", (e: any) => {
    console.log("exec code:", e && e.code);
    console.log("exec signal:", e && e.signal);
    console.log("exec killed:", e && e.killed);
    console.log("exec cmd:", e && e.cmd);
    console.log("exec keys:", e ? Object.keys(e).sort().join(",") : "");
    resolve();
  });
});

// #1935 — execFile failure Error carries code + cmd (file + args).
await new Promise<void>((resolve) => {
  cp.execFile("/bin/sh", ["-c", "exit 4"], (e: any) => {
    console.log("execFile code:", e && e.code);
    console.log("execFile cmd:", e && e.cmd);
    resolve();
  });
});

// #1936 — spawnSync result object fields (pid/signal/output + Buffer stdout).
const r = cp.spawnSync("/bin/echo", ["sync-hi"]);
console.log("spawnSync keys:", Object.keys(r).sort().join(","));
console.log("spawnSync status:", r.status);
console.log("spawnSync signal:", r.signal);
console.log("spawnSync pid typeof:", typeof r.pid);
console.log("spawnSync output len:", r.output.length);
console.log("spawnSync output[0]:", r.output[0]);
console.log("spawnSync stdout isBuffer:", Buffer.isBuffer(r.stdout));
console.log("spawnSync stdout:", r.stdout.toString().trim());

// #1937 — encoding option: utf8 yields strings, default yields Buffers.
const rb = cp.spawnSync("/bin/echo", ["enc"], { encoding: "utf8" });
console.log("spawnSync utf8 isString:", typeof rb.stdout === "string");
console.log("spawnSync utf8 stdout:", rb.stdout.trim());
console.log("execSync default isBuffer:", Buffer.isBuffer(cp.execSync("echo dflt")));
const es = cp.execSync("echo enc2", { encoding: "utf8" });
console.log("execSync utf8 isString:", typeof es === "string", es.trim());

// #1938 — execSync/execFileSync throw on a non-zero exit.
try {
  cp.execSync("exit 5");
  console.log("execSync did not throw (WRONG)");
} catch (e: any) {
  console.log("execSync threw status:", e.status);
  console.log("execSync threw keys:", Object.keys(e).sort().join(","));
}
try {
  cp.execFileSync("/bin/sh", ["-c", "exit 6"]);
  console.log("execFileSync did not throw (WRONG)");
} catch (e: any) {
  console.log("execFileSync threw status:", e.status);
}

console.log("errors done");
