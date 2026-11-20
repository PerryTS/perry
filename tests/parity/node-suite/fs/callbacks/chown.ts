import * as fs from "node:fs";
import * as os from "node:os";

const ROOT = "/tmp/perry_node_suite_fs_callback_chown";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const link = ROOT + "/link.txt";
fs.writeFileSync(p, "owner");
fs.symlinkSync(p, link);
const info = os.userInfo();

await new Promise<void>((resolve) => {
  fs.chown(p, info.uid, info.gid, (err) => {
    console.log("chown callback err:", err === null);
    resolve();
  });
});

const fd = fs.openSync(p, "r+");
await new Promise<void>((resolve) => {
  fs.fchown(fd, info.uid, info.gid, (err) => {
    console.log("fchown callback err:", err === null);
    resolve();
  });
});
fs.closeSync(fd);

await new Promise<void>((resolve) => {
  (fs as any).lchown(link, info.uid, info.gid, (err) => {
    console.log("lchown callback err:", err === null);
    resolve();
  });
});
console.log("chown callback content:", fs.readFileSync(p, "utf8"));
