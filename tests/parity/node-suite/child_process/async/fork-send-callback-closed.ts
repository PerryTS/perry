import { fork } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const childFile = join(tmpdir(), `perry-fork-send-${process.pid}.js`);
writeFileSync(
  childFile,
  "process.on('message', () => {}); setInterval(() => {}, 1000);",
);

const child = fork(childFile, [], { stdio: ["ignore", "ignore", "ignore", "ipc"] });

console.log("send length:", child.send.length);
const successReturn = child.send({ ok: true }, (err: any) => {
  console.log("send callback success err:", err === null ? "null" : err?.code ?? err?.name);
  child.disconnect();
  setTimeout(() => {
    const closedReturn = child.send({ afterDisconnect: true }, (closedErr: any) => {
      console.log("send callback closed err:", closedErr?.name, closedErr?.code);
      child.kill();
      try {
        unlinkSync(childFile);
      } catch {}
    });
    console.log("send return closed:", closedReturn);
  }, 25);
});

console.log("send return success:", successReturn);
