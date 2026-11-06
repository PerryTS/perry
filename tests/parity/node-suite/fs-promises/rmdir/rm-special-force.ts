import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_promises_rm_special_force";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const file = ROOT + "/file []{}() !-á.txt";
await fsp.writeFile(file, "x");
await fsp.rm(Buffer.from(file));
console.log("promises rm buffer special file removed:", !fs.existsSync(file));

const nested = ROOT + "/nested []/child";
await fsp.mkdir(nested, { recursive: true });
await fsp.writeFile(nested + "/a.txt", "a");
await fsp.rm(pathToFileURL(ROOT + "/nested []"), { recursive: true, force: true });
console.log("promises rm url recursive special removed:", !fs.existsSync(ROOT + "/nested []"));

await fsp.rm(ROOT + "/missing []", { force: true });
console.log("promises rm force missing ok:", !fs.existsSync(ROOT + "/missing []"));
