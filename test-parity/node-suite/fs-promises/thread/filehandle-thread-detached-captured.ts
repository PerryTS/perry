import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { spawn } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_thread_detached_captured";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const path = ROOT + "/input.txt";
await fsp.writeFile(path, "alpha");

const fh = await fsp.open(path, "r");
async function runCaptured(handle: fsp.FileHandle) {
  return await spawn(() => {
    let readFileCode = "ok";
    try {
      fs.readFileSync(handle as any);
    } catch (e: any) {
      readFileCode = `${e.code}:${e.syscall}`;
    }
    return { fd: (handle as any).fd, readFileCode };
  });
}

const worker = await runCaptured(fh);

const stats = await fh.stat();
console.log("captured filehandle worker fd:", worker.fd);
console.log("captured filehandle as fd:", worker.readFileCode);
console.log("captured original usable:", stats.isFile(), fh.fd >= 0);
await fh.close();
