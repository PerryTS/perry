import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_access_permission_errors";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const file = ROOT + "/readonly.txt";
await fsp.writeFile(file, "readonly");

const canCheckPermissions = process.platform !== "win32" && typeof process.getuid === "function" && process.getuid() !== 0;

function report(label: string, err: any) {
  console.log(label, "instance", err instanceof Error);
  console.log(label, "code", err && err.code);
  console.log(label, "errno-number", typeof (err && err.errno) === "number");
  console.log(label, "syscall", err && err.syscall);
  console.log(label, "path-ok", err && err.path === file);
}

if (canCheckPermissions) {
  try {
    fs.chmodSync(file, 0o400);
    try {
      await fsp.access(file, fs.constants.W_OK);
      console.log("promises access denied resolved");
    } catch (err: any) {
      report("promises access denied", err);
    }
  } finally {
    try { fs.chmodSync(file, 0o600); } catch (_e) {}
    try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
  }
} else {
  console.log("promises access permission denied skipped");
}
