import * as fs from "node:fs";
import { spawn } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fs_threaded_fd_semantics_collision_returned";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const mainPath = ROOT + "/main.txt";
const workerPath = ROOT + "/worker.txt";
fs.writeFileSync(mainPath, "MAIN");
fs.writeFileSync(workerPath, "WORK");

const mainFd = fs.openSync(mainPath, "r");
const workerFd = await spawn(() => fs.openSync(workerPath, "r"));

let returnedRead = "ok";
try {
  const returnedBuffer = Buffer.alloc(4);
  const returnedBytes = fs.readSync(workerFd, returnedBuffer, 0, 4, 0);
  returnedRead = returnedBuffer.subarray(0, returnedBytes).toString();
} catch (e: any) {
  returnedRead = `${e.code}:${e.syscall}`;
}

const mainBuffer = Buffer.alloc(4);
const mainBytes = fs.readSync(mainFd, mainBuffer, 0, 4, 0);

console.log("collision returned fd read:", returnedRead);
console.log("main local fd still reads:", mainBuffer.subarray(0, mainBytes).toString());

fs.closeSync(mainFd);
