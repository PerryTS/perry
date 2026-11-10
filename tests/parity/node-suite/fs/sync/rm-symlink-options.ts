import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_rm_symlink_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const targetDir = ROOT + "/target-dir";
const linkDir = ROOT + "/link-dir";
fs.mkdirSync(targetDir);
fs.writeFileSync(targetDir + "/keep.txt", "keep-dir-target");
fs.symlinkSync(targetDir, linkDir, "dir");
fs.rmSync(linkDir, { recursive: true });
console.log("rm symlink dir link removed:", !fs.existsSync(linkDir));
console.log("rm symlink dir target kept:", fs.readFileSync(targetDir + "/keep.txt", "utf8"));

const targetFile = ROOT + "/target-file.txt";
const linkFile = ROOT + "/link-file.txt";
fs.writeFileSync(targetFile, "keep-file-target");
fs.symlinkSync(targetFile, linkFile);
fs.rmSync(linkFile, { force: true });
console.log("rm symlink file link removed:", !fs.existsSync(linkFile));
console.log("rm symlink file target kept:", fs.readFileSync(targetFile, "utf8"));

fs.rmSync(ROOT + "/missing.txt", { force: true });
console.log("rm missing force ok:", true);
