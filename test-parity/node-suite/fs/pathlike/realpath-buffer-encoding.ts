import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_realpath_buffer_encoding";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const canonical = fs.realpathSync(ROOT, "utf8");
const expectedHex = Buffer.from(canonical).toString("hex");
const expectedBase64 = Buffer.from(canonical).toString("base64");
const rootBuffer = Buffer.from(ROOT);

console.log("sync utf8 string:", fs.realpathSync(rootBuffer, { encoding: "utf8" }) === canonical);
console.log("sync hex:", fs.realpathSync(rootBuffer, "hex") === expectedHex);
console.log("sync base64:", fs.realpathSync(ROOT, { encoding: "base64" }) === expectedBase64);
const syncBuffer = fs.realpathSync(rootBuffer, { encoding: "buffer" });
console.log("sync buffer isBuffer:", Buffer.isBuffer(syncBuffer));
console.log("sync buffer text:", syncBuffer.toString("utf8") === canonical);

await new Promise<void>((resolve) => {
  fs.realpath(rootBuffer, { encoding: "hex" }, (err, resolved) => {
    console.log("callback hex err null:", err === null);
    console.log("callback hex:", resolved === expectedHex);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.realpath(rootBuffer, "buffer", (err, resolved) => {
    console.log("callback buffer err null:", err === null);
    console.log("callback buffer isBuffer:", Buffer.isBuffer(resolved));
    console.log("callback buffer text:", resolved.toString("utf8") === canonical);
    resolve();
  });
});
