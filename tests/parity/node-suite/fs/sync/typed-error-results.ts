import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_sync_typed_error_results";
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

function report(label: string, err: any, expected: Expected) {
  console.log(label, "instance", err instanceof Error);
  console.log(label, "code", err && err.code);
  console.log(label, "errno-number", typeof (err && err.errno) === "number" && err.errno < 0);
  console.log(label, "syscall", err && err.syscall);
  console.log(label, "path-ok", pathOk(err, expected));
  console.log(label, "dest-ok", destOk(err, expected));
  console.log(label, "message-has-code", typeof err.message === "string" && err.message.startsWith(expected.code + ":"));
}

function capture(label: string, expected: Expected, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    report(label, err, expected);
  }
}

capture("mkdirSync existing", { code: "EEXIST", syscall: "mkdir", path: ROOT, noDest: true }, () => fs.mkdirSync(ROOT));

const mkdtempPrefix = MISSING_PARENT + "/temp-";
capture("mkdtempSync missing parent", { code: "ENOENT", syscall: "mkdtemp", pathPrefix: mkdtempPrefix, noDest: true }, () => fs.mkdtempSync(mkdtempPrefix));

const missingSource = ROOT + "/missing-source.txt";
const renameDest = ROOT + "/rename-dest.txt";
capture("renameSync missing source", { code: "ENOENT", syscall: "rename", path: missingSource, dest: renameDest }, () => fs.renameSync(missingSource, renameDest));

const existingRenameSource = ROOT + "/rename-source.txt";
const missingDestParent = MISSING_PARENT + "/renamed.txt";
fs.writeFileSync(existingRenameSource, "rename");
capture("renameSync missing dest parent", { code: "ENOENT", syscall: "rename", path: existingRenameSource, dest: missingDestParent }, () => fs.renameSync(existingRenameSource, missingDestParent));
try { fs.unlinkSync(existingRenameSource); } catch (_e) {}

const missingCpSource = ROOT + "/missing-cp-source.txt";
capture("cpSync missing source", { code: "ENOENT", syscall: "lstat", path: missingCpSource, noDest: true }, () => fs.cpSync(missingCpSource, ROOT + "/cp-dest.txt"));

capture("opendirSync missing path", { code: "ENOENT", syscall: "opendir", noPath: true, noDest: true }, () => fs.opendirSync(ROOT + "/missing-dir"));

const missingAccess = ROOT + "/missing-access.txt";
capture("accessSync missing", { code: "ENOENT", syscall: "access", path: missingAccess, noDest: true }, () => fs.accessSync(missingAccess));

const missingChmod = ROOT + "/missing-chmod.txt";
capture("chmodSync missing", { code: "ENOENT", syscall: "chmod", path: missingChmod, noDest: true }, () => fs.chmodSync(missingChmod, 0o600));

const missingChown = ROOT + "/missing-chown.txt";
capture("chownSync missing", { code: "ENOENT", syscall: "chown", path: missingChown, noDest: true }, () => fs.chownSync(missingChown, 0, 0));

const missingLchown = ROOT + "/missing-lchown.txt";
capture("lchownSync missing", { code: "ENOENT", syscall: "lchown", path: missingLchown, noDest: true }, () => fs.lchownSync(missingLchown, 0, 0));

const missingRm = ROOT + "/missing-rm.txt";
capture("rmSync missing", { code: "ENOENT", syscall: "lstat", path: missingRm, noDest: true }, () => fs.rmSync(missingRm));

const missingTruncate = ROOT + "/missing-truncate.txt";
capture("truncateSync missing", { code: "ENOENT", syscall: "open", path: missingTruncate, noDest: true }, () => fs.truncateSync(missingTruncate, 0));

capture("ftruncateSync EBADF", { code: "EBADF", syscall: "ftruncate", noPath: true, noDest: true }, () => fs.ftruncateSync(BAD_FD, 0));
capture("futimesSync EBADF", { code: "EBADF", syscall: "futime", noPath: true, noDest: true }, () => fs.futimesSync(BAD_FD, 1, 1));

if (typeof process.getuid === "function" && process.getuid() !== 0) {
  const pathChownPath = ROOT + "/chown-eperm.txt";
  fs.writeFileSync(pathChownPath, "owner");
  capture("chownSync EPERM", { code: "EPERM", syscall: "chown", path: pathChownPath, noDest: true }, () => fs.chownSync(pathChownPath, 0, 0));

  const chownPath = ROOT + "/fchown-eperm.txt";
  fs.writeFileSync(chownPath, "owner");
  const fd = fs.openSync(chownPath, "r+");
  capture("fchownSync EPERM", { code: "EPERM", syscall: "fchown", noPath: true, noDest: true }, () => fs.fchownSync(fd, 0, 0));
  fs.closeSync(fd);
} else {
  console.log("chownSync EPERM skipped");
  console.log("fchownSync EPERM skipped");
}
