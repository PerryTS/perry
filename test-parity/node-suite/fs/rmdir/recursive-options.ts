import * as fs from "node:fs";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_rmdir_recursive_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}

fs.mkdirSync(ROOT + "/sync/a/b", { recursive: true });
fs.writeFileSync(ROOT + "/sync/a/b/file.txt", "sync");
fs.rmdirSync(ROOT + "/sync", { recursive: true });
console.log("rmdirSync recursive removed:", !fs.existsSync(ROOT + "/sync"));

fs.mkdirSync(ROOT + "/callback/a/b", { recursive: true });
fs.writeFileSync(ROOT + "/callback/a/b/file.txt", "callback");
fs.rmdir(ROOT + "/callback", { recursive: true }, (err) => {
  console.log("rmdir callback recursive err:", err === null);
  console.log("rmdir callback recursive removed:", !fs.existsSync(ROOT + "/callback"));
});
