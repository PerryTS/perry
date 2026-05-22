import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_rm_symlink_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const targetDir = ROOT + "/target-dir";
const linkDir = ROOT + "/link-dir";
fs.mkdirSync(targetDir);
fs.writeFileSync(targetDir + "/keep.txt", "callback-keep");
fs.symlinkSync(targetDir, linkDir, "dir");

await new Promise<void>((resolve) => {
  fs.rm(linkDir, { recursive: true }, (err) => {
    console.log("rm callback symlink err:", err === null);
    console.log("rm callback symlink removed:", !fs.existsSync(linkDir));
    console.log("rm callback target kept:", fs.readFileSync(targetDir + "/keep.txt", "utf8"));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.rm(ROOT + "/missing", { force: true }, (err) => {
    console.log("rm callback missing force err:", err === null);
    resolve();
  });
});
