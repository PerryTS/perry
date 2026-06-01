import * as fs from "node:fs";
import { spawn } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fs_threaded_fd_semantics_collision_captured";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const mainPath = ROOT + "/main.txt";
const workerPath = ROOT + "/worker.txt";
fs.writeFileSync(mainPath, "MAIN");
fs.writeFileSync(workerPath, "WORK");

const fd = fs.openSync(mainPath, "r");
const worker = await spawn(() => {
  const localFd = fs.openSync(workerPath, "r");
  const localBuffer = Buffer.alloc(4);
  const localBytes = fs.readSync(localFd, localBuffer, 0, 4, 0);
  const localText = localBuffer.subarray(0, localBytes).toString();

  let capturedRead = "ok";
  try {
    const capturedBuffer = Buffer.alloc(4);
    const capturedBytes = fs.readSync(fd, capturedBuffer, 0, 4, 0);
    capturedRead = capturedBuffer.subarray(0, capturedBytes).toString();
  } catch (e: any) {
    capturedRead = `${e.code}:${e.syscall}`;
  } finally {
    fs.closeSync(localFd);
  }

  return { capturedRead, localText };
});

const mainBuffer = Buffer.alloc(4);
const mainBytes = fs.readSync(fd, mainBuffer, 0, 4, 0);

console.log("collision worker local read:", worker.localText);
console.log("collision captured fd read:", worker.capturedRead);
console.log("main fd still reads:", mainBuffer.subarray(0, mainBytes).toString());

fs.closeSync(fd);
