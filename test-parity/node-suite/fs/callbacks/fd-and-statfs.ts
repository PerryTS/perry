import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_fd_statfs";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "abcdef");
const fd = fs.openSync(p, "r+");

fs.fstat(fd, (err, stats) => {
  console.log("fstat callback err:", err === null);
  console.log("fstat callback size:", stats.size);
  fs.ftruncate(fd, 2, (err2) => {
    console.log("ftruncate callback err:", err2 === null);
    fs.fsync(fd, (err3) => {
      console.log("fsync callback err:", err3 === null);
      fs.closeSync(fd);
      console.log("ftruncate callback content:", fs.readFileSync(p, "utf8"));

      const link = ROOT + "/link.txt";
      fs.symlinkSync("file.txt", link);
      fs.lstat(link, (err4, lstats) => {
        console.log("lstat callback err:", err4 === null);
        console.log("lstat callback symlink:", lstats.isSymbolicLink());
        fs.statfs(ROOT, (err5, fsStats) => {
          console.log("statfs callback err:", err5 === null);
          console.log("statfs callback bsize positive:", fsStats.bsize > 0);
        });
      });
    });
  });
});
