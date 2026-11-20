import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_cp_rmdir";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/src/sub", { recursive: true });
fs.writeFileSync(ROOT + "/src/sub/a.txt", "A");

fs.cp(ROOT + "/src", ROOT + "/dst", { recursive: true }, (err) => {
  console.log("cp callback err:", err === null);
  console.log("cp callback content:", fs.readFileSync(ROOT + "/dst/sub/a.txt", "utf8"));
  const empty = ROOT + "/empty";
  fs.mkdirSync(empty);
  fs.rmdir(empty, (err2) => {
    console.log("rmdir callback err:", err2 === null);
    console.log("rmdir callback removed:", fs.existsSync(empty));
  });
});
