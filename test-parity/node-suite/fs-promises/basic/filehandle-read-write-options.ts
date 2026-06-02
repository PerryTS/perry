import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_read_write_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "abcdef");

const rh = await fsp.open(p, "r");
const readBuffer = Buffer.alloc(6, "_");
const rr = await rh.read({ buffer: readBuffer, offset: 1, length: 3, position: 2 });
console.log("fh read object bytes:", rr.bytesRead);
console.log("fh read object same buffer:", rr.buffer === readBuffer);
console.log("fh read object content:", readBuffer.toString("utf8"));
await rh.close();

const wh = await fsp.open(ROOT + "/out.txt", "w+");
const writeBuffer = Buffer.from("xyz123");
const wr = await wh.write(writeBuffer, 1, 3, 0);
console.log("fh write positional bytes:", wr.bytesWritten);
console.log("fh write positional same buffer:", wr.buffer === writeBuffer);
await wh.close();
console.log("fh write positional content:", await fsp.readFile(ROOT + "/out.txt", "utf8"));
