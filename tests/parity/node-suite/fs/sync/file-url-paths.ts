import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_file_url_paths";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file url.txt";
const copy = ROOT + "/copy url.txt";
const renamed = ROOT + "/renamed url.txt";
const link = ROOT + "/link url.txt";
const fileUrl = new URL("file://" + file.replace(" ", "%20"));
const copyUrl = new URL("file://" + copy.replace(" ", "%20"));
const renamedUrl = new URL("file://" + renamed.replace(" ", "%20"));
const linkUrl = new URL("file://" + link.replace(" ", "%20"));

fs.writeFileSync(fileUrl, "url-data");
console.log("url readFileSync:", fs.readFileSync(fileUrl, "utf8"));
fs.appendFileSync(fileUrl, "!");
console.log("url appendFileSync:", fs.readFileSync(file, "utf8"));
console.log("url stat isFile:", fs.statSync(fileUrl).isFile());
fs.accessSync(fileUrl);
console.log("url accessSync:", true);
fs.copyFileSync(fileUrl, copyUrl);
console.log("url copyFileSync:", fs.readFileSync(copy, "utf8"));
fs.renameSync(copyUrl, renamedUrl);
console.log("url renameSync exists:", fs.existsSync(renamed));
fs.symlinkSync(fileUrl, linkUrl);
console.log("url lstat symlink:", fs.lstatSync(linkUrl).isSymbolicLink());
console.log("url readlink suffix:", fs.readlinkSync(linkUrl).endsWith("file url.txt"));
console.log("url realpath suffix:", fs.realpathSync(linkUrl).endsWith("file url.txt"));
console.log("url statfs bsize:", fs.statfsSync(new URL("file://" + ROOT)).bsize > 0);
