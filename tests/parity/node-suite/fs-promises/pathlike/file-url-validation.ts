import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_promises_pathlike_file_url_validation";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const file = ROOT + "/file url.txt";
const fileUrl = pathToFileURL(file);
await fsp.writeFile(fileUrl, "url-data");
console.log("promises pathToFileURL read:", await fsp.readFile(fileUrl, "utf8"));
console.log("promises pathToFileURL stat:", (await fsp.stat(fileUrl)).isFile());
console.log("promises pathToFileURL readdir:", (await fsp.readdir(pathToFileURL(ROOT))).includes("file url.txt"));
console.log("promises pathToFileURL realpath suffix:", (await fsp.realpath(fileUrl)).endsWith("file url.txt"));
const made = await fsp.mkdtemp(pathToFileURL(ROOT + "/urltmp-"));
console.log("promises pathToFileURL mkdtemp prefix:", made.startsWith(ROOT + "/urltmp-"));
console.log("promises buffer path read:", await fsp.readFile(Buffer.from(file), "utf8"));

async function captureCode(label: string, makePromise: () => Promise<unknown>) {
  try {
    await makePromise();
    console.log(label, "resolved");
  } catch (err: any) {
    console.log(label, "code", err && err.code);
    console.log(label, "name", err && err.name);
  }
}

await captureCode("promises encoded slash URL", () => fsp.stat(new URL("file://" + ROOT + "/bad%2Fname")));
await captureCode("promises null byte URL", () => fsp.stat(new URL("file://" + ROOT + "/bad%00name")));
await captureCode("promises null byte string", () => fsp.stat(ROOT + "/bad\0name"));
