import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_writefile_typedarray";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const path = ROOT + "/typed.bin";
await fsp.writeFile(path, new Uint8Array([65, 66, 67]));
console.log("promises writeFile Uint8Array:", await fsp.readFile(path, "utf8"));

await fsp.appendFile(path, new Uint8Array([68, 69]));
console.log("promises appendFile Uint8Array:", await fsp.readFile(path, "utf8"));

const fh = await fsp.open(ROOT + "/filehandle.bin", "w+");
await fh.writeFile(new Uint8Array([70, 71, 72]));
await fh.close();
console.log("promises FileHandle.writeFile Uint8Array:", fs.readFileSync(ROOT + "/filehandle.bin", "utf8"));
