import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "perry-path-resolve-"));
const real = path.join(tmp, "real");
const link = path.join(tmp, "link");

try {
  fs.mkdirSync(real);
  fs.mkdirSync(path.join(real, "child"));
  fs.symlinkSync(real, link, "dir");

  const lexical = path.resolve(link, "child");
  const physical = fs.realpathSync(path.join(link, "child"));

  console.log("resolve keeps symlink:", lexical.endsWith("/link/child"));
  console.log("realpath follows symlink:", physical.endsWith("/real/child"));
  console.log("resolve differs from realpath:", lexical !== physical);
} catch (err: any) {
  console.log("symlink unsupported:", err?.code || err?.name);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
