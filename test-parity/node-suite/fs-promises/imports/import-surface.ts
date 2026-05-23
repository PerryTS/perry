import * as fsp from "node:fs/promises";
import { readFile, writeFile, readdir, stat, rm, lstat, cp, truncate, mkdtemp, readlink, open, statfs, utimes, lutimes, opendir, chmod, chown, lchown, glob, watch } from "node:fs/promises";

console.log("namespace object:", fsp !== null && typeof fsp === "object");
console.log("namespace readFile:", typeof fsp.readFile);
console.log("namespace writeFile:", typeof fsp.writeFile);
console.log("namespace readdir:", typeof fsp.readdir);
console.log("named readFile:", typeof readFile);
console.log("named writeFile:", typeof writeFile);
console.log("named readdir:", typeof readdir);
console.log("named stat:", typeof stat);
console.log("named rm:", typeof rm);
console.log("named chown:", typeof chown);
console.log("named lchown:", typeof lchown);
console.log("namespace chown:", typeof fsp.chown);
console.log("namespace lchown:", typeof (fsp as any).lchown);

console.log("named chmod:", typeof chmod);
console.log("named cp:", typeof cp);
console.log("named truncate:", typeof truncate);
console.log("named mkdtemp:", typeof mkdtemp);
console.log("named readlink:", typeof readlink);
console.log("named open:", typeof open);
console.log("named statfs:", typeof statfs);
console.log("named utimes:", typeof utimes);
console.log("named lutimes:", typeof lutimes);
console.log("named opendir:", typeof opendir);
console.log("named glob:", typeof glob);
console.log("named watch:", typeof watch);
