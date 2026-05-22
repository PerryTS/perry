import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cb_mutate";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
await new Promise<void>((resolve) => {
  fs.mkdir(ROOT, { recursive: true }, (err) => {
    console.log("mkdir err null:", err === null);
    resolve();
  });
});
fs.writeFileSync(ROOT + "/src.txt", "move");
await new Promise<void>((resolve) => {
  fs.copyFile(ROOT + "/src.txt", ROOT + "/copy.txt", (err) => {
    console.log("copy err null:", err === null);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.rename(ROOT + "/copy.txt", ROOT + "/renamed.txt", (err) => {
    console.log("rename err null:", err === null);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.unlink(ROOT + "/src.txt", (err) => {
    console.log("unlink err null:", err === null);
    resolve();
  });
});
console.log("renamed content:", fs.readFileSync(ROOT + "/renamed.txt", "utf8"));
console.log("src gone:", !fs.existsSync(ROOT + "/src.txt"));
await new Promise<void>((resolve) => {
  fs.rm(ROOT, { recursive: true, force: true }, (err) => {
    console.log("rm err null:", err === null);
    resolve();
  });
});
console.log("root gone:", !fs.existsSync(ROOT));
