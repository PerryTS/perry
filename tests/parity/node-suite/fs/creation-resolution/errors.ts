import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_creation_resolution";
const MISSING_PARENT = ROOT + "/missing-parent";

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const regularFile = ROOT + "/regular.txt";
fs.writeFileSync(regularFile, "regular");

type Expected = {
  code: string;
  syscall: string;
  path?: string;
  pathPrefix?: string;
  pathSuffix?: string;
  noDest?: boolean;
};

function pathOk(err: any, expected: Expected): boolean {
  if (expected.path !== undefined) return err.path === expected.path;
  if (expected.pathPrefix !== undefined) return typeof err.path === "string" && err.path.startsWith(expected.pathPrefix);
  if (expected.pathSuffix !== undefined) return typeof err.path === "string" && err.path.endsWith(expected.pathSuffix);
  return true;
}

function report(label: string, err: any, expected: Expected) {
  console.log(label, "instance", err instanceof Error);
  console.log(label, "code", err && err.code);
  console.log(label, "errno-number", typeof (err && err.errno) === "number" && err.errno < 0);
  console.log(label, "syscall", err && err.syscall);
  console.log(label, "path-ok", pathOk(err, expected));
  console.log(label, "dest-ok", expected.noDest ? err.dest === undefined : true);
}

function capture(label: string, expected: Expected, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    report(label, err, expected);
  }
}

async function captureCallback(label: string, expected: Expected, start: (cb: (err: any) => void) => void) {
  await new Promise<void>((resolve) => {
    start((err: any) => {
      report(label, err, expected);
      resolve();
    });
  });
}

capture("mkdirSync existing", { code: "EEXIST", syscall: "mkdir", path: ROOT, noDest: true }, () => fs.mkdirSync(ROOT));
capture("mkdirSync missing parent", { code: "ENOENT", syscall: "mkdir", path: MISSING_PARENT + "/dir", noDest: true }, () => fs.mkdirSync(MISSING_PARENT + "/dir"));
capture("mkdtempSync missing parent", { code: "ENOENT", syscall: "mkdtemp", pathPrefix: MISSING_PARENT + "/tmp-", noDest: true }, () => fs.mkdtempSync(MISSING_PARENT + "/tmp-"));
capture("realpathSync missing", { code: "ENOENT", syscall: "lstat", pathSuffix: "/missing-realpath", noDest: true }, () => fs.realpathSync(ROOT + "/missing-realpath"));
capture("readlinkSync regular file", { code: "EINVAL", syscall: "readlink", path: regularFile, noDest: true }, () => fs.readlinkSync(regularFile));
capture("readlinkSync missing", { code: "ENOENT", syscall: "readlink", path: ROOT + "/missing-link", noDest: true }, () => fs.readlinkSync(ROOT + "/missing-link"));

await captureCallback("mkdir callback existing", { code: "EEXIST", syscall: "mkdir", path: ROOT, noDest: true }, (cb) => fs.mkdir(ROOT, cb));
await captureCallback("mkdir callback missing parent", { code: "ENOENT", syscall: "mkdir", path: MISSING_PARENT + "/cb-dir", noDest: true }, (cb) => fs.mkdir(MISSING_PARENT + "/cb-dir", cb));
await captureCallback("mkdtemp callback missing parent", { code: "ENOENT", syscall: "mkdtemp", pathPrefix: MISSING_PARENT + "/cbtmp-", noDest: true }, (cb) => fs.mkdtemp(MISSING_PARENT + "/cbtmp-", cb));
await captureCallback("realpath callback missing", { code: "ENOENT", syscall: "lstat", pathSuffix: "/missing-realpath-cb", noDest: true }, (cb) => fs.realpath(ROOT + "/missing-realpath-cb", cb));
await captureCallback("readlink callback regular file", { code: "EINVAL", syscall: "readlink", path: regularFile, noDest: true }, (cb) => fs.readlink(regularFile, cb));
await captureCallback("readlink callback missing", { code: "ENOENT", syscall: "readlink", path: ROOT + "/missing-link-cb", noDest: true }, (cb) => fs.readlink(ROOT + "/missing-link-cb", cb));
