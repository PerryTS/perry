import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_dataview_writefile";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const ab = new ArrayBuffer(4);
const view = new Uint8Array(ab);
view[0] = 65;
view[1] = 66;
view[2] = 67;
view[3] = 68;
const dv = new DataView(ab);

const path = ROOT + "/dataview.bin";
await fsp.writeFile(path, dv);
console.log("promises writeFile DataView:", await fsp.readFile(path, "utf8"));

await fsp.appendFile(path, dv);
console.log("promises appendFile DataView:", await fsp.readFile(path, "utf8"));

const fh = await fsp.open(ROOT + "/filehandle.bin", "w+");
await fh.writeFile(dv);
await fh.close();
console.log("promises FileHandle.writeFile DataView:", await fsp.readFile(ROOT + "/filehandle.bin", "utf8"));
