import * as fsp from "node:fs/promises";
import { spawn } from "perry/thread";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_thread_detached_returned";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const path = ROOT + "/input.txt";
await fsp.writeFile(path, "beta");

const fh = await fsp.open(path, "r");
const returned: any = await spawn(() => fh as any);

let statCode = "ok";
try {
  await returned.stat();
} catch (e: any) {
  statCode = `${e.code}:${e.syscall}`;
}

console.log("returned filehandle fd:", returned.fd);
console.log("returned filehandle stat:", statCode);
console.log("returned original still open:", fh.fd >= 0);
await returned.close();
console.log("returned close fd:", returned.fd);
await fh.close();
