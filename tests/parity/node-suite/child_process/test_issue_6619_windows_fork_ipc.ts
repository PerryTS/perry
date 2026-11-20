// #6619 — Windows fork IPC uses a libuv-framed named pipe. Exercise both
// directions against a real Node child so handle inheritance and framing are
// covered together.
import { fork } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const helper = join(tmpdir(), `perry-windows-ipc-${process.pid}.js`);
writeFileSync(
  helper,
  "process.once('message', (message) => process.send({ echo: message.value }, () => process.disconnect()));",
);

const child = fork(helper);
child.once("error", (error: any) => {
  console.log("error", error.code, error.errno, error.syscall);
  rmSync(helper, { force: true });
});
child.once("message", (message: any) => {
  console.log("message", message.echo);
});
child.once("close", (code, signal) => {
  console.log("close", code, signal);
  rmSync(helper, { force: true });
});
console.log("connected", child.connected);
console.log("send", child.send({ value: 6619 }));
