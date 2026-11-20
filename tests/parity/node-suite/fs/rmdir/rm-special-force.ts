import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_rm_special_force";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file []{}() !-á.txt";
fs.writeFileSync(file, "x");
fs.rmSync(Buffer.from(file));
console.log("rmSync buffer special file removed:", !fs.existsSync(file));

const nested = ROOT + "/nested []/child";
fs.mkdirSync(nested, { recursive: true });
fs.writeFileSync(nested + "/a.txt", "a");
fs.rmSync(pathToFileURL(ROOT + "/nested []"), { recursive: true, force: true });
console.log("rmSync url recursive special removed:", !fs.existsSync(ROOT + "/nested []"));

fs.rmSync(ROOT + "/missing []", { force: true });
console.log("rmSync force missing ok:", !fs.existsSync(ROOT + "/missing []"));

const callback = ROOT + "/callback special/file.txt";
fs.mkdirSync(ROOT + "/callback special", { recursive: true });
fs.writeFileSync(callback, "c");
fs.rm(Buffer.from(ROOT + "/callback special"), { recursive: true, force: true }, (err) => {
  console.log("rm callback buffer recursive err:", err === null);
  console.log("rm callback buffer recursive removed:", !fs.existsSync(ROOT + "/callback special"));
});
