// #3079 — child_process command/file/args input validation.
// #3316 — subprocess.send callback + closed-channel semantics.
//
// Byte-for-byte vs `node --experimental-strip-types`. The fork()ed child runs
// under the configured interpreter (default `node`), so the IPC half is exact
// when node is on PATH.
import * as cp from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// ---------------------------------------------------------------------------
// #3079 — setup-time validation throws synchronously, before any spawn.
// ---------------------------------------------------------------------------
function probe(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "OK");
  } catch (e: any) {
    console.log(label, "|", e.name, "|", e.code, "|", e.message);
  }
}

probe("exec missing", () => cp.exec(undefined as any));
probe("exec number", () => cp.exec(123 as any));
probe("execSync missing", () => cp.execSync(undefined as any));
probe("execSync number", () => cp.execSync(123 as any));
probe("execFile missing", () => cp.execFile(undefined as any));
probe("execFile number", () => cp.execFile(123 as any));
probe("execFileSync missing", () => cp.execFileSync(undefined as any));
probe("execFileSync number", () => cp.execFileSync(123 as any));
probe("execFileSync args string", () => cp.execFileSync("echo", "-n" as any));
probe("spawnSync missing", () => cp.spawnSync(undefined as any));
probe("spawnSync number", () => cp.spawnSync(123 as any));
probe("spawnSync args string", () => cp.spawnSync("echo", "-n" as any));
probe("spawnSync args number", () => cp.spawnSync("echo", 5 as any));
probe("spawn missing", () => cp.spawn(undefined as any));
probe("spawn number", () => cp.spawn(123 as any));
probe("spawn args string", () => cp.spawn("echo", "-n" as any));

// Valid forms still work (args may be null/undefined/object/array).
console.log("spawnSync null-args status:", cp.spawnSync("echo", null as any).status);
console.log("spawnSync array-args status:", cp.spawnSync("echo", ["hi"]).status);

// ---------------------------------------------------------------------------
// #3316 — subprocess.send callback + closed-channel semantics.
// ---------------------------------------------------------------------------
const childSrc = "process.on('message', () => {});\n";
const childPath = path.join(os.tmpdir(), "perry_send_child_" + process.pid + ".mjs");
fs.writeFileSync(childPath, childSrc);

const child = cp.fork(childPath, [], {
  stdio: ["ignore", "ignore", "ignore", "ipc"],
});

console.log("send.length:", child.send.length);

await new Promise<void>((resolve) => {
  // Successful send: returns true, callback fires async with null.
  const r1 = child.send({ ok: true }, (err: any) => {
    console.log("cb1:", err === null ? "null" : (err && err.code));
    // Now close the channel and send again.
    child.disconnect();
    const r2 = child.send({ after: true }, (err2: any) => {
      console.log("cb2:", err2 && err2.name, "|", err2 && err2.code, "|", err2 && err2.message);
      child.kill();
      resolve();
    });
    console.log("send2:", r2);
  });
  console.log("send1:", r1);
});

fs.unlinkSync(childPath);
console.log("done");
