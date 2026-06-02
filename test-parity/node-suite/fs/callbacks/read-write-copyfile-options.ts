import * as fs from "node:fs";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_callback_rw_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const p = ROOT + "/file.txt";
fs.writeFile(p, "alpha", (err) => {
  console.log("writeFile options err:", err === null);
  fs.appendFile(p, " beta", (err2) => {
    console.log("appendFile options err:", err2 === null);
    fs.readFile(p, (err3, data) => {
      console.log("readFile buffer err:", err3 === null);
      console.log("readFile buffer isBuffer:", Buffer.isBuffer(data));
      console.log("readFile buffer text:", data.toString("utf8"));
      fs.readFile(p, "utf8", (err4, text) => {
        console.log("readFile utf8 err:", err4 === null);
        console.log("readFile utf8 text:", text);
        const src = ROOT + "/src.txt";
        const dst = ROOT + "/dst.txt";
        fs.writeFileSync(src, "new");
        fs.writeFileSync(dst, "old");
        fs.copyFile(src, dst, (err5) => {
          console.log("copyFile callback err:", err5 === null);
          console.log("copyFile callback overwrites:", fs.readFileSync(dst, "utf8"));
        });
      });
    });
  });
});
