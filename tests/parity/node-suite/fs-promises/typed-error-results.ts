import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_typed_error_results";
const MISSING_PARENT = ROOT + "/missing-parent";

try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

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
}

async function capture(label: string, expected: Expected, makePromise: () => Promise<unknown>) {
  let promise: Promise<unknown>;
  try {
    promise = makePromise();
    console.log(label, "is-promise", typeof (promise as any).then === "function");
  } catch (err: any) {
    console.log(label, "is-promise", false);
    report(label, err, expected);
    return;
  }
  try {
    const value = await promise;
    console.log(label, "resolved", value === undefined);
  } catch (err: any) {
    report(label, err, expected);
  }
}

await capture("mkdir existing", { code: "EEXIST", syscall: "mkdir", path: ROOT, noDest: true }, () => fsp.mkdir(ROOT));

const mkdtempPrefix = MISSING_PARENT + "/temp-";
await capture("mkdtemp missing parent", { code: "ENOENT", syscall: "mkdtemp", pathPrefix: mkdtempPrefix, noDest: true }, () => fsp.mkdtemp(mkdtempPrefix));

const missingSource = ROOT + "/missing-source.txt";
const renameDest = ROOT + "/rename-dest.txt";
await capture("rename missing source", { code: "ENOENT", syscall: "rename", path: missingSource, dest: renameDest }, () => fsp.rename(missingSource, renameDest));

const existingRenameSource = ROOT + "/rename-source.txt";
const missingDestParent = MISSING_PARENT + "/renamed.txt";
await fsp.writeFile(existingRenameSource, "rename");
await capture("rename missing dest parent", { code: "ENOENT", syscall: "rename", path: existingRenameSource, dest: missingDestParent }, () => fsp.rename(existingRenameSource, missingDestParent));
try { fs.unlinkSync(existingRenameSource); } catch (_e) {}

const missingCpSource = ROOT + "/missing-cp-source.txt";
await capture("cp missing source", { code: "ENOENT", syscall: "lstat", path: missingCpSource, noDest: true }, () => fsp.cp(missingCpSource, ROOT + "/cp-dest.txt"));

await capture("opendir missing path", { code: "ENOENT", syscall: "opendir", path: ROOT + "/missing-dir", noDest: true }, () => fsp.opendir(ROOT + "/missing-dir"));

const missingAccess = ROOT + "/missing-access.txt";
await capture("access missing", { code: "ENOENT", syscall: "access", path: missingAccess, noDest: true }, () => fsp.access(missingAccess));

const missingChmod = ROOT + "/missing-chmod.txt";
await capture("chmod missing", { code: "ENOENT", syscall: "chmod", path: missingChmod, noDest: true }, () => fsp.chmod(missingChmod, 0o600));

const missingChown = ROOT + "/missing-chown.txt";
await capture("chown missing", { code: "ENOENT", syscall: "chown", path: missingChown, noDest: true }, () => fsp.chown(missingChown, 0, 0));

const missingLchown = ROOT + "/missing-lchown.txt";
await capture("lchown missing", { code: "ENOENT", syscall: "lchown", path: missingLchown, noDest: true }, () => fsp.lchown(missingLchown, 0, 0));

const missingRm = ROOT + "/missing-rm.txt";
await capture("rm missing", { code: "ENOENT", syscall: "lstat", path: missingRm, noDest: true }, () => fsp.rm(missingRm));

const missingTruncate = ROOT + "/missing-truncate.txt";
await capture("truncate missing", { code: "ENOENT", syscall: "open", path: missingTruncate, noDest: true }, () => fsp.truncate(missingTruncate, 0));

if (typeof process.getuid === "function" && process.getuid() !== 0) {
  const pathChownPath = ROOT + "/chown-eperm.txt";
  await fsp.writeFile(pathChownPath, "owner");
  await capture("chown EPERM", { code: "EPERM", syscall: "chown", path: pathChownPath, noDest: true }, () => fsp.chown(pathChownPath, 0, 0));
} else {
  console.log("chown EPERM skipped");
}
