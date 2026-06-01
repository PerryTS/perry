import * as fs from "node:fs";
import { spawn } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fs_threaded_fd_semantics_worker_returned";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const path = ROOT + "/input.txt";
fs.writeFileSync(path, "worker");

const workerFd = await spawn(() => fs.openSync(path, "r"));
let mainFstat = "ok";
try {
  fs.fstatSync(workerFd);
} catch (e: any) {
  mainFstat = `${e.code}:${e.syscall}`;
}

console.log("worker returned fd type:", typeof workerFd);
console.log("worker returned fd main fstat:", mainFstat);
