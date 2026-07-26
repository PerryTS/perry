import * as fs from "node:fs";

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
