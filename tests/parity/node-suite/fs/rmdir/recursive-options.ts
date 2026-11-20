import * as fs from "node:fs";
import { rmdir as rmdirPromise } from "node:fs/promises";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_rmdir_recursive_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}

try {
  fs.rmdirSync(ROOT, { recursive: true });
} catch (err) {
  console.log("rmdirSync recursive error:", err?.code);
}

try {
  fs.rmdir(ROOT, { recursive: true }, () => {});
} catch (err) {
  console.log("rmdir callback recursive error:", err?.code);
}

try {
  await fs.promises.rmdir(ROOT, { recursive: true });
} catch (err) {
  console.log("rmdir promises recursive error:", err?.code);
}

try {
  await fs.promises.rmdir();
} catch (err) {
  console.log("rmdir promises missing path error:", err?.code);
}

try {
  await rmdirPromise();
} catch (err) {
  console.log("rmdir promises namespace missing path error:", err?.code);
}

fs.mkdirSync(ROOT);
fs.rmdirSync(ROOT, {});
console.log("rmdir empty options removed:", !fs.existsSync(ROOT));
