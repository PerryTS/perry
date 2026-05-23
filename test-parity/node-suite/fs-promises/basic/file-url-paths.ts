import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_file_url_paths";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const file = ROOT + "/file url.txt";
const copy = ROOT + "/copy url.txt";
const renamed = ROOT + "/renamed url.txt";
const link = ROOT + "/link url.txt";
const fileUrl = new URL("file://" + file.replace(" ", "%20"));
const copyUrl = new URL("file://" + copy.replace(" ", "%20"));
const renamedUrl = new URL("file://" + renamed.replace(" ", "%20"));
const linkUrl = new URL("file://" + link.replace(" ", "%20"));

await fsp.writeFile(fileUrl, "url-data");
console.log("promises url readFile:", await fsp.readFile(fileUrl, "utf8"));
await fsp.appendFile(fileUrl, "!");
console.log("promises url appendFile:", fs.readFileSync(file, "utf8"));
console.log("promises url stat isFile:", (await fsp.stat(fileUrl)).isFile());
await fsp.access(fileUrl);
console.log("promises url access:", true);
await fsp.copyFile(fileUrl, copyUrl);
console.log("promises url copyFile:", await fsp.readFile(copy, "utf8"));
await fsp.rename(copyUrl, renamedUrl);
console.log("promises url rename exists:", fs.existsSync(renamed));
await fsp.symlink(fileUrl, linkUrl);
console.log("promises url lstat symlink:", (await fsp.lstat(linkUrl)).isSymbolicLink());
console.log("promises url readlink suffix:", (await fsp.readlink(linkUrl)).endsWith("file url.txt"));
console.log("promises url realpath suffix:", (await fsp.realpath(linkUrl)).endsWith("file url.txt"));
