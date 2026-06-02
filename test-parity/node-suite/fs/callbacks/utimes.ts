import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_utimes";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "time");

fs.utimes(p, 1_600_000_111, 1_600_000_222, (err) => {
  console.log("utimes callback err:", err === null);
  console.log("utimes callback mtime:", Math.round(fs.statSync(p).mtimeMs / 1000));
  const fd = fs.openSync(p, "r+");
  fs.futimes(fd, 1_600_000_333, 1_600_000_444, (err2) => {
    console.log("futimes callback err:", err2 === null);
    console.log("futimes callback mtime:", Math.round(fs.fstatSync(fd).mtimeMs / 1000));
    fs.closeSync(fd);
  });
});
