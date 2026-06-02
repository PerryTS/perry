import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdir_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const nested = ROOT + "/a/b/c";
fs.mkdirSync(nested, { recursive: true });
console.log("mkdir options recursive dir:", fs.statSync(nested).isDirectory());

const numeric = ROOT + "/numeric";
fs.mkdirSync(numeric, 0o700);
console.log("mkdir options numeric mode:", (fs.statSync(numeric).mode & 0o777).toString(8));

const stringMode = ROOT + "/string";
fs.mkdirSync(stringMode, "700" as any);
console.log("mkdir options string mode:", (fs.statSync(stringMode).mode & 0o777).toString(8));

const urlDir = new URL("file://" + ROOT + "/url%20dir");
fs.mkdirSync(urlDir, { recursive: true });
console.log("mkdir options file url:", fs.statSync(ROOT + "/url dir").isDirectory());
