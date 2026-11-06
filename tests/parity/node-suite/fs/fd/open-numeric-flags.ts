import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_open_numeric_flags";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const c = fs.constants;

const created = ROOT + "/created.txt";
let fd = fs.openSync(created, c.O_CREAT | c.O_WRONLY | c.O_TRUNC);
fs.writeSync(fd, "numeric");
fs.closeSync(fd);
console.log("open numeric create write:", fs.readFileSync(created, "utf8"));

fd = fs.openSync(created, c.O_RDWR | c.O_APPEND);
fs.writeSync(fd, "-append");
fs.closeSync(fd);
console.log("open numeric append:", fs.readFileSync(created, "utf8"));

const exclusive = ROOT + "/exclusive.txt";
fd = fs.openSync(exclusive, c.O_CREAT | c.O_WRONLY | c.O_EXCL);
fs.writeSync(fd, "exclusive");
fs.closeSync(fd);
console.log("open numeric exclusive content:", fs.readFileSync(exclusive, "utf8"));
let exclusiveFailed = false;
try { const existing = fs.openSync(exclusive, c.O_CREAT | c.O_WRONLY | c.O_EXCL); exclusiveFailed = existing < 0; } catch (_e) { exclusiveFailed = true; }
console.log("open numeric exclusive existing failed:", exclusiveFailed);

let missingFailed = false;
try { const missing = fs.openSync(ROOT + "/missing.txt", c.O_WRONLY); missingFailed = missing < 0; } catch (_e) { missingFailed = true; }
console.log("open numeric write missing failed:", missingFailed);

fs.open(ROOT + "/callback.txt", c.O_CREAT | c.O_WRONLY | c.O_TRUNC, (err, cbfd) => {
  console.log("open callback numeric err:", err === null);
  fs.writeSync(cbfd, "callback");
  fs.closeSync(cbfd);
  console.log("open callback numeric content:", fs.readFileSync(ROOT + "/callback.txt", "utf8"));
});
