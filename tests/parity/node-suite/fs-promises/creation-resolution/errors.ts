import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_creation_resolution";
const MISSING_PARENT = ROOT + "/missing-parent";

try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const regularFile = ROOT + "/regular.txt";
fs.writeFileSync(regularFile, "regular");

type Expected = {
  code: string;
  syscall: string;
  path?: string;
  pathPrefix?: string;
  noDest?: boolean;
};

function pathOk(err: any, expected: Expected): boolean {
  if (expected.path !== undefined) return err.path === expected.path;
  if (expected.pathPrefix !== undefined) return typeof err.path === "string" && err.path.startsWith(expected.pathPrefix);
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
    await promise;
    console.log(label, "resolved");
  } catch (err: any) {
    report(label, err, expected);
  }
}

await capture("mkdir existing", { code: "EEXIST", syscall: "mkdir", path: ROOT, noDest: true }, () => fsp.mkdir(ROOT));
await capture("mkdir missing parent", { code: "ENOENT", syscall: "mkdir", path: MISSING_PARENT + "/dir", noDest: true }, () => fsp.mkdir(MISSING_PARENT + "/dir"));
await capture("mkdtemp missing parent", { code: "ENOENT", syscall: "mkdtemp", pathPrefix: MISSING_PARENT + "/tmp-", noDest: true }, () => fsp.mkdtemp(MISSING_PARENT + "/tmp-"));
await capture("realpath missing", { code: "ENOENT", syscall: "realpath", path: ROOT + "/missing-realpath", noDest: true }, () => fsp.realpath(ROOT + "/missing-realpath"));
await capture("readlink regular file", { code: "EINVAL", syscall: "readlink", path: regularFile, noDest: true }, () => fsp.readlink(regularFile));
await capture("readlink missing", { code: "ENOENT", syscall: "readlink", path: ROOT + "/missing-link", noDest: true }, () => fsp.readlink(ROOT + "/missing-link"));
