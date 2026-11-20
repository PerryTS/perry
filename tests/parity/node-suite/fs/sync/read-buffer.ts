import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_buffer";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/buf.txt";
fs.writeFileSync(p, "buffer-data");
const data = fs.readFileSync(p);
console.log("buffer isBuffer:", Buffer.isBuffer(data));
console.log("buffer length:", data.length);
console.log("buffer text:", data.toString("utf8"));
