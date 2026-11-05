// #1933 — child_process.fork() + IPC channel: send / 'message' / disconnect /
// connected. The forked module runs under the configured interpreter (default
// `node`, which speaks the NODE_CHANNEL_FD IPC protocol), so this is byte-for-
// byte vs `node --experimental-strip-types` when node is on PATH.
import * as cp from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// A tiny IPC echo child: ping -> pong, bye -> disconnect.
const childSrc = [
  "process.on('message', (m) => {",
  "  if (m && m.cmd === 'ping') process.send({ pong: m.n });",
  "  if (m && m.cmd === 'bye') process.disconnect();",
  "});",
].join("\n");
const childPath = path.join(os.tmpdir(), "perry_fork_child_" + process.pid + ".mjs");
fs.writeFileSync(childPath, childSrc);

const child = cp.fork(childPath);
console.log("fork typeof:", typeof child);
console.log("send typeof:", typeof child.send);
console.log("disconnect typeof:", typeof child.disconnect);
console.log("connected initially:", child.connected);

await new Promise<void>((resolve) => {
  const got: number[] = [];
  child.on("message", (m: any) => {
    got.push(m.pong);
    if (got.length === 3) {
      console.log("pongs:", got.join(","));
      child.send({ cmd: "bye" });
    }
  });
  child.on("exit", () => {
    console.log("child exited; connected:", child.connected);
    resolve();
  });
  child.send({ cmd: "ping", n: 1 });
  child.send({ cmd: "ping", n: 2 });
  child.send({ cmd: "ping", n: 3 });
});

fs.unlinkSync(childPath);
console.log("fork done");
