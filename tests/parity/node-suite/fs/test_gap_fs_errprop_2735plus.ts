// Gap test: node:fs error propagation across sync / promise forms for the
// two-path mutators (rename/copyFile/link/symlink) and path-based truncate.
// Covers #2735, #2737, #2738, #2740, #2743. We only assert `err.code` (and the
// dangling-symlink success), never volatile message text.
import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const dir = "/tmp/perry_fs_errprop_2735";
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });

const src = dir + "/src.txt";
const existing = dir + "/existing.txt";
const missing = dir + "/missing.txt";
const missingParent = dir + "/nope/child.txt";

fs.writeFileSync(src, "hi");
fs.writeFileSync(existing, "x");

function syncCode(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": OK");
  } catch (e: any) {
    console.log(label + ": " + e.code);
  }
}

async function asyncCode(label: string, fn: () => Promise<unknown>): Promise<void> {
  try {
    await fn();
    console.log(label + ": OK");
  } catch (e: any) {
    console.log(label + ": " + e.code);
  }
}

// rename (#2735)
syncCode("rename missing src sync", () => fs.renameSync(missing, dir + "/d.txt"));
syncCode("rename missing dest sync", () => fs.renameSync(src, missingParent));
// copyFile (#2737)
syncCode("copyFile missing src sync", () => fs.copyFileSync(missing, dir + "/c.txt"));
syncCode("copyFile missing dest sync", () => fs.copyFileSync(src, missingParent));
// link (#2738)
syncCode("link missing src sync", () => fs.linkSync(missing, dir + "/l.txt"));
syncCode("link existing dest sync", () => fs.linkSync(src, existing));
// symlink (#2740)
syncCode("symlink missing dest sync", () => fs.symlinkSync(missing, missingParent));
syncCode("symlink existing dest sync", () => fs.symlinkSync(missing, existing));
syncCode("symlink dangling sync", () => fs.symlinkSync(missing, dir + "/dangling.txt"));
// truncate (#2743)
syncCode("truncate missing sync", () => fs.truncateSync(missing, 0));
syncCode("truncate dir sync", () => fs.truncateSync(dir, 0));

async function main(): Promise<void> {
  await asyncCode("rename missing src promise", () => fsp.rename(missing, dir + "/d2.txt"));
  await asyncCode("rename missing dest promise", () => fsp.rename(src, missingParent));
  await asyncCode("copyFile missing src promise", () => fsp.copyFile(missing, dir + "/c2.txt"));
  await asyncCode("copyFile missing dest promise", () => fsp.copyFile(src, missingParent));
  await asyncCode("link missing src promise", () => fsp.link(missing, dir + "/l2.txt"));
  await asyncCode("link existing dest promise", () => fsp.link(src, existing));
  await asyncCode("symlink missing dest promise", () => fsp.symlink(missing, missingParent));
  await asyncCode("symlink existing dest promise", () => fsp.symlink(missing, existing));
  await asyncCode("symlink dangling promise", () => fsp.symlink(missing, dir + "/dangling2.txt"));
  await asyncCode("truncate missing promise", () => fsp.truncate(missing, 0));
  await asyncCode("truncate dir promise", () => fsp.truncate(dir, 0));

  // Successful rename moves the file.
  const moveFrom = dir + "/move-from.txt";
  const moveTo = dir + "/move-to.txt";
  fs.writeFileSync(moveFrom, "data");
  fs.renameSync(moveFrom, moveTo);
  console.log("rename success existsTo=" + fs.existsSync(moveTo) + " existsFrom=" + fs.existsSync(moveFrom));

  fs.rmSync(dir, { recursive: true, force: true });
}

main();
