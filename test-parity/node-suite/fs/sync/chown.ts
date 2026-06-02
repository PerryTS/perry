import * as fs from "node:fs";
import * as os from "node:os";

const ROOT = "/tmp/perry_node_suite_fs_chown";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
const link = ROOT + "/link.txt";
fs.writeFileSync(p, "owner");
fs.symlinkSync(p, link);
const info = os.userInfo();

fs.chownSync(p, info.uid, info.gid);
console.log("chownSync content:", fs.readFileSync(p, "utf8"));

const fd = fs.openSync(p, "r+");
fs.fchownSync(fd, info.uid, info.gid);
fs.closeSync(fd);
console.log("fchownSync exists:", fs.existsSync(p));

(fs as any).lchownSync(link, info.uid, info.gid);
console.log("lchownSync link:", fs.lstatSync(link).isSymbolicLink());
