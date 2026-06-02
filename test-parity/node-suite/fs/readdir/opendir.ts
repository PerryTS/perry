import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/a.txt", "A");
fs.writeFileSync(ROOT + "/b.txt", "B");
fs.mkdirSync(ROOT + "/dir");

const dir = fs.opendirSync(ROOT);
console.log("dir path:", dir.path === ROOT);
const first = dir.readSync();
const second = dir.readSync();
const third = dir.readSync();
const done = dir.readSync();
dir.closeSync();
const names = [first.name, second.name, third.name].sort();
console.log("opendir names:", names.join(","));
console.log("opendir done null:", done === null);
console.log("opendir predicates:", first.isFile() || first.isDirectory());

fs.opendir(ROOT, (err, asyncDir) => {
  console.log("opendir callback err:", err === null);
  asyncDir.read((readErr, entry) => {
    console.log("dir read callback err:", readErr === null);
    console.log("dir read callback name type:", typeof entry.name);
    asyncDir.close((closeErr) => {
      console.log("dir close callback err:", closeErr === null);
    });
  });
});
