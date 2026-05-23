import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fsp_realpath_buffer_encoding";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const canonical = fs.realpathSync(ROOT, "utf8");
const expectedHex = Buffer.from(canonical).toString("hex");
const expectedBase64 = Buffer.from(canonical).toString("base64");
const rootBuffer = Buffer.from(ROOT);

console.log("promises utf8 string:", await fsp.realpath(rootBuffer, { encoding: "utf8" }) === canonical);
console.log("promises hex:", await fsp.realpath(rootBuffer, "hex") === expectedHex);
console.log("promises base64:", await fsp.realpath(ROOT, { encoding: "base64" }) === expectedBase64);
const resolvedBuffer = await fsp.realpath(rootBuffer, { encoding: "buffer" });
console.log("promises buffer isBuffer:", Buffer.isBuffer(resolvedBuffer));
console.log("promises buffer text:", resolvedBuffer.toString("utf8") === canonical);
