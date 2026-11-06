import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

// @ts-ignore
process.emitWarning = function () {};

const ROOT = "/tmp/perry_node_suite_fsp_write_copyfile_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const p = ROOT + "/file.txt";
const copyFile = (fsp as any)["copyFile"];
await fsp.writeFile(p, "one");
await fsp.appendFile(p, " two");
console.log("promises write append:", await fsp.readFile(p, "utf8"));

const src = ROOT + "/src.txt";
const dst = ROOT + "/dst.txt";
await fsp.writeFile(src, "source");
await fsp.writeFile(dst, "dest");
try { await copyFile(src, dst, fs.constants.COPYFILE_EXCL); } catch (_e) {}
console.log("promises copyFile excl keeps existing:", await fsp.readFile(dst, "utf8"));
await copyFile(src, dst);
console.log("promises copyFile overwrites:", await fsp.readFile(dst, "utf8"));

const tmp = await fsp.mkdtemp(ROOT + "/tmp-");
console.log("promises mkdtemp prefix:", tmp.indexOf(ROOT + "/tmp-") === 0);
console.log("promises realpath suffix:", (await fsp.realpath(tmp)).endsWith(tmp.split('/').pop() as string));
