import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_readlink_encoding_pathlike";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/target [] á.txt", "x");
await fsp.symlink("target [] á.txt", ROOT + "/link [] á.txt");

const target = await fsp.readlink(Buffer.from(ROOT + "/link [] á.txt"));
console.log("promises readlink buffer path:", target);

const bufferTarget = await fsp.readlink(Buffer.from(ROOT + "/link [] á.txt"), { encoding: "buffer" });
console.log("promises readlink buffer encoding is buffer:", Buffer.isBuffer(bufferTarget));
console.log("promises readlink buffer encoding value:", bufferTarget.toString());
