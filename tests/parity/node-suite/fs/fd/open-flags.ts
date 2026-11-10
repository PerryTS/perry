import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_open_flags";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";

let fd = fs.openSync(p, "wx");
console.log("open wx fd:", fd >= 0);
fs.writeSync(fd, "new");
fs.closeSync(fd);
console.log("open wx content:", fs.readFileSync(p, "utf8"));

let wxExistingFailed = false;
try { const fdExisting = fs.openSync(p, "wx"); wxExistingFailed = fdExisting < 0; } catch (_e) { wxExistingFailed = true; }
console.log("open wx existing failed:", wxExistingFailed);

fd = fs.openSync(p, "a+");
fs.writeSync(fd, " append");
fs.closeSync(fd);
console.log("open a+ content:", fs.readFileSync(p, "utf8"));

const ax = ROOT + "/append-exclusive.txt";
fd = fs.openSync(ax, "ax");
fs.writeSync(fd, "ax");
fs.closeSync(fd);
console.log("open ax content:", fs.readFileSync(ax, "utf8"));
let axExistingFailed = false;
try { const axExisting = fs.openSync(ax, "ax"); axExistingFailed = axExisting < 0; } catch (_e) { axExistingFailed = true; }
console.log("open ax existing failed:", axExistingFailed);
