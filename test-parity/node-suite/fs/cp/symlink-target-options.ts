import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cp_symlink_target_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function setup(name: string) {
  const base = ROOT + "/" + name;
  fs.mkdirSync(base, { recursive: true });
  fs.writeFileSync(base + "/target.txt", "hello");
  fs.mkdirSync(base + "/from");
  fs.symlinkSync("../target.txt", base + "/from/rel_link");
  fs.symlinkSync(base + "/target.txt", base + "/from/abs_link");
  return base;
}

const syncBase = setup("sync");
fs.cpSync(syncBase + "/from", syncBase + "/to", { recursive: true });
const syncRelTarget = fs.readlinkSync(syncBase + "/to/rel_link");
console.log("cp sync relative symlink resolved:", syncRelTarget === syncBase + "/target.txt");
console.log("cp sync abs symlink preserved target:", fs.readlinkSync(syncBase + "/to/abs_link") === syncBase + "/target.txt");
fs.rmSync(syncBase + "/from", { recursive: true, force: true });
console.log("cp sync copied symlink survives source removal:", fs.readFileSync(syncBase + "/to/rel_link", "utf8"));

const verbatimBase = setup("verbatim");
fs.cpSync(verbatimBase + "/from", verbatimBase + "/to", { recursive: true, verbatimSymlinks: true });
console.log("cp sync verbatim symlink target:", fs.readlinkSync(verbatimBase + "/to/rel_link"));

const callbackBase = setup("callback");
fs.cp(callbackBase + "/from", callbackBase + "/to", { recursive: true }, (err) => {
  console.log("cp callback symlink err:", err === null);
  console.log("cp callback relative symlink resolved:", fs.readlinkSync(callbackBase + "/to/rel_link") === callbackBase + "/target.txt");
});
