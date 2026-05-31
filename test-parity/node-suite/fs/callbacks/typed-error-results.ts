import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callbacks_typed_error_results";
const MISSING_PARENT = ROOT + "/missing-parent";
const BAD_FD = 987654321;

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

type Expected = {
  code: string;
  syscall: string;
  path?: string;
  pathPrefix?: string;
  dest?: string;
  noPath?: boolean;
  noDest?: boolean;
};

function pathOk(err: any, expected: Expected): boolean {
  if (expected.noPath) return err.path === undefined;
  if (expected.path !== undefined) return err.path === expected.path;
  if (expected.pathPrefix !== undefined) return typeof err.path === "string" && err.path.startsWith(expected.pathPrefix);
  return true;
}

function destOk(err: any, expected: Expected): boolean {
  if (expected.noDest) return err.dest === undefined;
  if (expected.dest !== undefined) return err.dest === expected.dest;
  return true;
}

function report(label: string, err: any, value: any, expected: Expected) {
  console.log(label, "instance", err instanceof Error);
  console.log(label, "code", err && err.code);
  console.log(label, "errno-number", typeof (err && err.errno) === "number" && err.errno < 0);
  console.log(label, "syscall", err && err.syscall);
  console.log(label, "path-ok", pathOk(err, expected));
  console.log(label, "dest-ok", destOk(err, expected));
  console.log(label, "value-undefined", value === undefined);
}

async function capture(label: string, expected: Expected, invoke: (cb: (err: any, value?: any) => void) => void) {
  await new Promise<void>((resolve) => {
    invoke((err: any, value?: any) => {
      report(label, err, value, expected);
      resolve();
    });
  });
}

await capture("mkdir existing", { code: "EEXIST", syscall: "mkdir", path: ROOT, noDest: true }, (cb) => (fs as any).mkdir(ROOT, cb));

const mkdtempPrefix = MISSING_PARENT + "/temp-";
await capture("mkdtemp missing parent", { code: "ENOENT", syscall: "mkdtemp", pathPrefix: mkdtempPrefix, noDest: true }, (cb) => (fs as any).mkdtemp(mkdtempPrefix, cb));

const missingSource = ROOT + "/missing-source.txt";
const renameDest = ROOT + "/rename-dest.txt";
await capture("rename missing source", { code: "ENOENT", syscall: "rename", path: missingSource, dest: renameDest }, (cb) => (fs as any).rename(missingSource, renameDest, cb));

const existingRenameSource = ROOT + "/rename-source.txt";
const missingDestParent = MISSING_PARENT + "/renamed.txt";
fs.writeFileSync(existingRenameSource, "rename");
await capture("rename missing dest parent", { code: "ENOENT", syscall: "rename", path: existingRenameSource, dest: missingDestParent }, (cb) => (fs as any).rename(existingRenameSource, missingDestParent, cb));
try { fs.unlinkSync(existingRenameSource); } catch (_e) {}

const missingCpSource = ROOT + "/missing-cp-source.txt";
await capture("cp missing source", { code: "ENOENT", syscall: "lstat", path: missingCpSource, noDest: true }, (cb) => (fs as any).cp(missingCpSource, ROOT + "/cp-dest.txt", cb));

await capture("opendir missing path", { code: "ENOENT", syscall: "opendir", path: ROOT + "/missing-dir", noDest: true }, (cb) => (fs as any).opendir(ROOT + "/missing-dir", cb));

await capture("ftruncate EBADF", { code: "EBADF", syscall: "ftruncate", noPath: true, noDest: true }, (cb) => (fs as any).ftruncate(BAD_FD, 0, cb));
await capture("futimes EBADF", { code: "EBADF", syscall: "futime", noPath: true, noDest: true }, (cb) => (fs as any).futimes(BAD_FD, 1, 1, cb));

if (typeof process.getuid === "function" && process.getuid() !== 0) {
  const chownPath = ROOT + "/fchown-eperm.txt";
  fs.writeFileSync(chownPath, "owner");
  const fd = fs.openSync(chownPath, "r+");
  await capture("fchown EPERM", { code: "EPERM", syscall: "fchown", noPath: true, noDest: true }, (cb) => (fs as any).fchown(fd, 0, 0, cb));
  fs.closeSync(fd);
} else {
  console.log("fchown EPERM skipped");
}
