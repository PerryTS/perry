import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readfile_fd_position";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "Hello World");

const fd = fs.openSync(p, "r");
const head = Buffer.alloc(5);
console.log("readSync bytes:", fs.readSync(fd, head, 0, 5, null));
console.log("readSync head:", head.toString("utf8"));
console.log("readFileSync fd rest:", fs.readFileSync(fd).toString("utf8"));
fs.closeSync(fd);

await new Promise<void>((resolve) => {
  fs.open(p, "r", (openErr, fd2) => {
    const buf = Buffer.alloc(5);
    console.log("open err null:", openErr === null);
    fs.read(fd2, buf, 0, 5, null, (readErr, bytes) => {
      console.log("callback read err null:", readErr === null);
      console.log("callback read bytes:", bytes);
      console.log("callback read head:", buf.toString("utf8"));
      fs.readFile(fd2, (fileErr, data) => {
        console.log("readFile fd err null:", fileErr === null);
        console.log("readFile fd rest:", data.toString("utf8"));
        fs.closeSync(fd2);
        resolve();
      });
    });
  });
});
