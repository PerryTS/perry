import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_rmdir_pathlike_special_chars";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const special = ROOT + "/dir []{}() !-á";
fs.mkdirSync(special);
fs.rmdirSync(Buffer.from(special));
console.log("rmdirSync buffer special removed:", !fs.existsSync(special));

const urlDir = ROOT + "/url dir";
fs.mkdirSync(urlDir);
fs.rmdirSync(pathToFileURL(urlDir));
console.log("rmdirSync url removed:", !fs.existsSync(urlDir));

const callbackDir = ROOT + "/callback []{}";
fs.mkdirSync(callbackDir);
fs.rmdir(Buffer.from(callbackDir), (err) => {
  console.log("rmdir callback buffer err:", err === null);
  console.log("rmdir callback buffer removed:", !fs.existsSync(callbackDir));
});
