import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_write_append_flush_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const path = ROOT + "/promises.txt";
await fsp.writeFile(path, "write", { flush: true });
await fsp.appendFile(path, "-append", { flush: true });
console.log("promises write append flush content:", await fsp.readFile(path, "utf8"));

const fh = await fsp.open(ROOT + "/filehandle.txt", "w+");
await fh.writeFile("first", { flush: true });
await fh.writeFile("-second", { flush: true });
await fh.close();
console.log("promises filehandle writeFile keeps position:", await fsp.readFile(ROOT + "/filehandle.txt", "utf8"));
