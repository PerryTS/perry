import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_more_mutators";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "abcdef");

fs.truncate(p, 3, (err) => {
  console.log("truncate callback err:", err === null);
  console.log("truncate callback content:", fs.readFileSync(p, "utf8"));
  fs.chmod(p, 0o600, (err2) => {
    console.log("chmod callback err:", err2 === null);
    console.log("chmod callback mode:", (fs.statSync(p).mode & 0o777).toString(8));
    const link = ROOT + "/link.txt";
    fs.symlinkSync("file.txt", link);
    fs.readlink(link, (err3, target) => {
      console.log("readlink callback err:", err3 === null);
      console.log("readlink callback target:", target);
      fs.realpath(p, (err4, real) => {
        console.log("realpath callback err:", err4 === null);
        console.log("realpath callback suffix:", real.endsWith("/file.txt"));
        fs.mkdtemp(ROOT + "/tmp-", (err5, made) => {
          console.log("mkdtemp callback err:", err5 === null);
          console.log("mkdtemp callback prefix:", made.indexOf(ROOT + "/tmp-") === 0);
        });
      });
    });
  });
});
