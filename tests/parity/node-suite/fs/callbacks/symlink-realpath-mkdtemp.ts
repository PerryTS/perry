import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_links_mkdtemp";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/target.txt";
const link = ROOT + "/link.txt";
fs.writeFileSync(p, "target");
fs.symlinkSync(p, link);

await new Promise<void>((resolve) => {
  fs.lstat(link, (err, st) => {
    console.log("lstat callback err:", err === null);
    console.log("lstat callback symlink:", st.isSymbolicLink());
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.readlink(link, (err, target) => {
    console.log("readlink callback err:", err === null);
    console.log("readlink callback target suffix:", target.endsWith("target.txt"));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.realpath(link, (err, resolved) => {
    console.log("realpath callback err:", err === null);
    console.log("realpath callback target suffix:", resolved.endsWith("target.txt"));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.mkdtemp(ROOT + "/tmp-", (err, dir) => {
    console.log("mkdtemp callback err:", err === null);
    console.log("mkdtemp callback prefix:", dir.indexOf(ROOT + "/tmp-") === 0);
    console.log("mkdtemp callback exists:", fs.existsSync(dir));
    resolve();
  });
});
