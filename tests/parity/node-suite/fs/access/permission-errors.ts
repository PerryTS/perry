import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_access_permission_errors";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/readonly.txt";
fs.writeFileSync(file, "readonly");

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
      fs.accessSync(file, fs.constants.W_OK);
      console.log("accessSync denied no-throw");
    } catch (err: any) {
      report("accessSync denied", err);
    }

    await new Promise<void>((resolve) => {
      fs.access(file, fs.constants.W_OK, (err) => {
        report("access callback denied", err);
        resolve();
      });
    });
  } finally {
    try { fs.chmodSync(file, 0o600); } catch (_e) {}
    try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
  }
} else {
  console.log("access permission denied skipped");
}
