// Issue #1780: child_process.spawn() now returns a real streaming ChildProcess
// — an EventEmitter (`spawn`/`exit`/`close`) whose `stdout`/`stderr` are
// Readable streams (`data`/`end`) — instead of the spawnSync result object whose
// `.stdout` was a string (so `child.stdout.on(...)` threw). Also adds
// `execFile` (callback) and `execFileSync`.
//
// Probes shape + event delivery deterministically: no pid / raw exit codes /
// stderr text that vary per host. Byte-for-byte vs `node --experimental-strip-types`.
import { spawn, execFile, execFileSync } from "node:child_process";

// ── spawn: stdout `data` accumulates, then `exit` fires ──
await new Promise<void>((resolve) => {
  const sp = spawn("/bin/echo", ["spawn-hello"]);
  console.log("spawn typeof:", typeof sp);
  console.log("spawn typeof stdout:", typeof sp.stdout);
  console.log("spawn typeof kill:", typeof sp.kill);
  let buf = "";
  sp.stdout.on("data", (chunk: any) => {
    buf += chunk.toString();
  });
  sp.on("exit", () => {
    console.log("spawn stdout:", buf.trim());
    resolve();
  });
});

// ── ChildProcess shape (probe types only — values vary per run) ──
{
  const sp = spawn("/bin/echo", ["shape"]);
  console.log("typeof channel:", typeof sp.channel);
  console.log("typeof connected:", typeof sp.connected);
  console.log("typeof exitCode:", typeof sp.exitCode);
  console.log("typeof killed:", typeof sp.killed);
  console.log("typeof pid:", typeof sp.pid);
  console.log("typeof signalCode:", typeof sp.signalCode);
  console.log("typeof spawnargs:", typeof sp.spawnargs);
  console.log("spawnargs isArray:", Array.isArray(sp.spawnargs));
  console.log("typeof spawnfile:", typeof sp.spawnfile);
  console.log("typeof stdin:", typeof sp.stdin);
  console.log("typeof stdout:", typeof sp.stdout);
  console.log("typeof stderr:", typeof sp.stderr);
  console.log("typeof stdio:", typeof sp.stdio);
  console.log("typeof uid:", typeof sp.uid);
  console.log("typeof gid:", typeof sp.gid);
  console.log("typeof kill:", typeof sp.kill);
  console.log("typeof send:", typeof sp.send);
  console.log("typeof disconnect:", typeof sp.disconnect);
  console.log("typeof ref:", typeof sp.ref);
  console.log("typeof unref:", typeof sp.unref);
  await new Promise<void>((resolve) => {
    sp.on("exit", () => resolve());
  });
}

// ── events: spawn / exit / close all fire ──
{
  const sp = spawn("/bin/echo", ["events"]);
  let sawSpawn = false;
  let sawExit = false;
  let sawClose = false;
  sp.on("spawn", () => {
    sawSpawn = true;
  });
  sp.on("exit", () => {
    sawExit = true;
  });
  sp.on("close", () => {
    sawClose = true;
  });
  await new Promise<void>((resolve) => {
    sp.on("close", () => resolve());
  });
  console.log("event spawn fired:", sawSpawn);
  console.log("event exit fired:", sawExit);
  console.log("event close fired:", sawClose);
}

// ── execFile(file, args, callback) ──
const ef = await new Promise<string>((resolve, reject) => {
  execFile("/bin/echo", ["execFile-hello"], (err: any, stdout: any) => {
    if (err) reject(err);
    else resolve(String(stdout).trim());
  });
});
console.log("execFile stdout:", ef);

// ── execFileSync(file, args) ──
console.log(
  "execFileSync stdout:",
  execFileSync("/bin/echo", ["execFileSync-hello"]).toString().trim(),
);
