import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_write_buffer";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.bin";
await fsp.writeFile(p, Buffer.from([0x61, 0x62]));
await fsp.appendFile(p, Buffer.from([0x63]));
console.log("promises write buffer text:", await fsp.readFile(p, "utf8"));
const h = await fsp.open(ROOT + "/fh.bin", "w+");
await h.writeFile(Buffer.from("hi"));
await h.appendFile(Buffer.from("!"));
await h.close();
console.log("filehandle write buffer:", await fsp.readFile(ROOT + "/fh.bin", "utf8"));
