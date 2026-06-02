import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_promises_copyfile_pathlike_flags";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const src = ROOT + "/src []{}.txt";
const dst = ROOT + "/dst []{}.txt";
await fsp.writeFile(src, "source");
await fsp.copyFile(Buffer.from(src), pathToFileURL(dst));
console.log("promises copyFile buffer url:", await fsp.readFile(dst, "utf8"));

await fsp.writeFile(src, "new");
try { await fsp.copyFile(src, dst, fs.constants.COPYFILE_EXCL); } catch (_e) {}
console.log("promises copyFile excl kept dest:", await fsp.readFile(dst, "utf8"));
