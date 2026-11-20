import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_rename_pathlike";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const a = ROOT + "/a [] á.txt";
const b = ROOT + "/b [] á.txt";
const c = ROOT + "/c [] á.txt";
fs.writeFileSync(a, "sync");
fs.renameSync(Buffer.from(a), Buffer.from(b));
console.log("renameSync pathlike source gone:", !fs.existsSync(a));
console.log("renameSync pathlike dest content:", fs.readFileSync(b, "utf8"));

fs.rename(Buffer.from(b), Buffer.from(c), (err) => {
  console.log("rename callback pathlike err:", err === null);
  console.log("rename callback pathlike source gone:", !fs.existsSync(b));
  console.log("rename callback pathlike dest content:", fs.readFileSync(c, "utf8"));
});
