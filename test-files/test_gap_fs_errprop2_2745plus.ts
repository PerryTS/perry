// Gap test: node:fs error propagation across sync / promise forms for
// utimes/lutimes (#2745), chmod/chown/lchown (#2746), rm/rmdir (#2747),
// access (#2748), and readlink (#2733). We assert only `err.code` and
// `err.syscall` (and success markers), never volatile message text.
//
// Default-import of node:fs/promises is a pre-existing gap, so use the
// namespace import form.
import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const dir = "/tmp/perry_fs_errprop2_2745";
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });

const file = dir + "/file.txt";
const missing = dir + "/missing.txt";
const nonemptyDir = dir + "/sub";
const link = dir + "/link";

fs.writeFileSync(file, "hi");
fs.mkdirSync(nonemptyDir);
fs.writeFileSync(nonemptyDir + "/inner.txt", "x");
fs.symlinkSync(file, link);

function syncCode(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": OK");
  } catch (e: any) {
    console.log(label + ": " + e.code + " " + e.syscall);
  }
}

async function asyncCode(label: string, fn: () => Promise<unknown>): Promise<void> {
  try {
    await fn();
    console.log(label + ": OK");
  } catch (e: any) {
    console.log(label + ": " + e.code + " " + e.syscall);
  }
}

// --- sync forms ---
// utimes / lutimes (#2745)
syncCode("utimes missing sync", () => fs.utimesSync(missing, 1, 2));
syncCode("lutimes missing sync", () => fs.lutimesSync(missing, 1, 2));
syncCode("utimes ok sync", () => fs.utimesSync(file, 1, 2));
// chmod / chown / lchown (#2746)
syncCode("chmod missing sync", () => fs.chmodSync(missing, 0o600));
syncCode("chmod ok sync", () => fs.chmodSync(file, 0o644));
syncCode("chown missing sync", () => fs.chownSync(missing, 0, 0));
syncCode("lchown missing sync", () => fs.lchownSync(missing, 0, 0));
// rm / rmdir (#2747)
syncCode("rmdir missing sync", () => fs.rmdirSync(missing));
syncCode("rmdir file sync", () => fs.rmdirSync(file));
syncCode("rmdir nonempty sync", () => fs.rmdirSync(nonemptyDir));
syncCode("rm missing sync", () => fs.rmSync(missing));
syncCode("rm force missing sync", () => fs.rmSync(missing, { force: true }));
syncCode("rm dir nonrecursive sync", () => fs.rmSync(nonemptyDir));
// access (#2748)
syncCode("access missing sync", () => fs.accessSync(missing));
syncCode("access W_OK ok sync", () => fs.accessSync(file, fs.constants.W_OK));
// readlink (#2733)
syncCode("readlink regular sync", () => fs.readlinkSync(file));
syncCode("readlink missing sync", () => fs.readlinkSync(missing));
console.log("readlink ok sync target=" + (fs.readlinkSync(link) === file));

async function main(): Promise<void> {
  await asyncCode("utimes missing promise", () => fsp.utimes(missing, 1, 2));
  await asyncCode("lutimes missing promise", () => fsp.lutimes(missing, 1, 2));
  await asyncCode("chmod missing promise", () => fsp.chmod(missing, 0o600));
  await asyncCode("chown missing promise", () => fsp.chown(missing, 0, 0));
  await asyncCode("lchown missing promise", () => fsp.lchown(missing, 0, 0));
  await asyncCode("rmdir missing promise", () => fsp.rmdir(missing));
  await asyncCode("rmdir nonempty promise", () => fsp.rmdir(nonemptyDir));
  await asyncCode("rm missing promise", () => fsp.rm(missing));
  await asyncCode("rm force missing promise", () => fsp.rm(missing, { force: true }));
  await asyncCode("rm dir nonrecursive promise", () => fsp.rm(nonemptyDir));
  await asyncCode("access missing promise", () => fsp.access(missing));
  await asyncCode("readlink regular promise", () => fsp.readlink(file));
  await asyncCode("readlink missing promise", () => fsp.readlink(missing));

  const linkTarget = await fsp.readlink(link);
  console.log("readlink ok promise target=" + (linkTarget === file));

  fs.rmSync(dir, { recursive: true, force: true });
}

main();
