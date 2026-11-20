import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_as_pathlike";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const writePath = ROOT + "/write.txt";
const writeHandle = await fsp.open(writePath, "w+");
await fsp.writeFile(writeHandle, "data");
await writeHandle.close();
console.log("promises writeFile FileHandle:", fs.readFileSync(writePath, "utf8"));

const readPath = ROOT + "/read.txt";
await fsp.writeFile(readPath, "read-data");
const readHandle = await fsp.open(readPath, "r");
const readBuffer = await fsp.readFile(readHandle);
await readHandle.close();
console.log("promises readFile FileHandle:", readBuffer.toString());

const appendPath = ROOT + "/append.txt";
const appendHandle = await fsp.open(appendPath, "w+");
await fsp.appendFile(appendHandle, "data");
await fsp.appendFile(appendHandle, "data");
await appendHandle.close();
console.log("promises appendFile FileHandle:", fs.readFileSync(appendPath, "utf8"));
