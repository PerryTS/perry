import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_writefile_typedarray";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const syncPath = ROOT + "/sync.bin";
fs.writeFileSync(syncPath, new Uint8Array([65, 66, 67]));
console.log("writeFileSync Uint8Array:", fs.readFileSync(syncPath, "utf8"));

fs.appendFileSync(syncPath, new Uint8Array([68, 69]));
console.log("appendFileSync Uint8Array:", fs.readFileSync(syncPath, "utf8"));

const callbackPath = ROOT + "/callback.bin";
fs.writeFile(callbackPath, new Uint8Array([70, 71]), (err) => {
  console.log("writeFile callback Uint8Array err:", err === null);
  fs.appendFile(callbackPath, new Uint8Array([72]), (err2) => {
    console.log("appendFile callback Uint8Array err:", err2 === null);
    console.log("callback Uint8Array content:", fs.readFileSync(callbackPath, "utf8"));
  });
});
