import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cb_readdir";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/b.txt", "b");
fs.writeFileSync(ROOT + "/a.txt", "a");
await new Promise<void>((resolve) => {
  fs.access(ROOT + "/a.txt", fs.constants.F_OK, (err) => {
    console.log("access err null:", err === null);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.readdir(ROOT, (err, names) => {
    const sorted = names.slice().sort();
    console.log("readdir err null:", err === null);
    console.log("readdir names:", sorted.join(","));
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.stat(ROOT + "/a.txt", (err, st) => {
    console.log("stat err null:", err === null);
    console.log("stat file size:", st.size);
    console.log("stat file isFile:", st.isFile());
    resolve();
  });
});
