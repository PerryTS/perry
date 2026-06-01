import * as fs from "node:fs";
import { spawn } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fs_threaded_fd_semantics_spawn_captured";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const path = ROOT + "/input.txt";
fs.writeFileSync(path, "alpha");

const fd = fs.openSync(path, "r");
const worker = await spawn(() => {
  try {
    fs.fstatSync(fd);
    return "ok";
  } catch (e: any) {
    return `${e.code}:${e.syscall}`;
  }
});

console.log("spawn captured fd fstat:", worker);
console.log("main fd still valid:", fs.fstatSync(fd).isFile());
fs.closeSync(fd);
