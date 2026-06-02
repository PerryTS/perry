import * as fs from "node:fs";
import { parallelMap } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fs_threaded_fd_semantics_parallelmap_read";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const path = ROOT + "/input.txt";
fs.writeFileSync(path, "abcdef");

const fd = fs.openSync(path, "r");
const results = parallelMap([0, 1, 2, 3], () => {
  try {
    fs.readSync(fd, Buffer.alloc(1), 0, 1, 0);
    return "ok";
  } catch (e: any) {
    return `${e.code}:${e.syscall}`;
  }
});

console.log("parallelMap captured fd read:", results.join(","));
console.log("main fd read bytes:", fs.readSync(fd, Buffer.alloc(1), 0, 1, 0));
fs.closeSync(fd);
