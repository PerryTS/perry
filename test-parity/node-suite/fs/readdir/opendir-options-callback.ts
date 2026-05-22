import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir_options_callback";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/b.txt", "b");
fs.writeFileSync(ROOT + "/a.txt", "a");

fs.opendir(ROOT, { bufferSize: 1 }, (err, dir) => {
  console.log("opendir options callback err:", err === null);
  console.log("opendir options callback path:", dir.path === ROOT);
  const first = dir.readSync();
  const second = dir.readSync();
  const entries = [first.name + ":" + first.isFile(), second.name + ":" + second.isFile()].sort();
  console.log("opendir options dirents:", entries.join(","));
  dir.close((closeErr) => {
    console.log("opendir options close callback err:", closeErr === null);
  });
});
