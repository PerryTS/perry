import { existsSync, readFileSync, writeFileSync, mkdirSync, readdirSync, chownSync, lchownSync, link, symlink, stat, readlink, realpath, mkdtemp, openAsBlob } from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_named";
try { mkdirSync(ROOT); } catch (_e) {}
writeFileSync(ROOT + "/a.txt", "alpha");
console.log("exists imported:", existsSync(ROOT + "/a.txt"));
console.log("read imported:", readFileSync(ROOT + "/a.txt", "utf8"));
const names = readdirSync(ROOT).slice().sort();
console.log("readdir imported length:", names.length);
console.log("readdir imported first:", names[0]);
console.log("named chownSync type:", typeof chownSync);
console.log("named lchownSync type:", typeof lchownSync);

console.log("named link type:", typeof link);
console.log("named symlink type:", typeof symlink);
console.log("named stat type:", typeof stat);
console.log("named readlink type:", typeof readlink);
console.log("named realpath type:", typeof realpath);
console.log("named mkdtemp type:", typeof mkdtemp);
console.log("named openAsBlob type:", typeof openAsBlob, openAsBlob.length);
