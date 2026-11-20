import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { pathToFileURL } from "node:url";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fs_promises_rmdir_recursive_pathlike";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

await fsp.mkdir(ROOT + "/recursive/a/b", { recursive: true });
await fsp.writeFile(ROOT + "/recursive/a/b/file.txt", "x");
await fsp.rmdir(ROOT + "/recursive", { recursive: true });
console.log("promises rmdir recursive removed:", !fs.existsSync(ROOT + "/recursive"));

const bufferDir = ROOT + "/buffer []{} !";
await fsp.mkdir(bufferDir);
await fsp.rmdir(Buffer.from(bufferDir));
console.log("promises rmdir buffer removed:", !fs.existsSync(bufferDir));

const urlDir = ROOT + "/url dir";
await fsp.mkdir(urlDir);
await fsp.rmdir(pathToFileURL(urlDir));
console.log("promises rmdir url removed:", !fs.existsSync(urlDir));
