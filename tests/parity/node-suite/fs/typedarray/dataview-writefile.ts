import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_dataview_writefile";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const ab = new ArrayBuffer(4);
const view = new Uint8Array(ab);
view[0] = 65;
view[1] = 66;
view[2] = 67;
view[3] = 68;
const dv = new DataView(ab);

const syncPath = ROOT + "/dataview.bin";
fs.writeFileSync(syncPath, dv);
console.log("writeFileSync DataView:", fs.readFileSync(syncPath, "utf8"));

fs.appendFileSync(syncPath, dv);
console.log("appendFileSync DataView:", fs.readFileSync(syncPath, "utf8"));

const cbPath = ROOT + "/callback-dataview.bin";
fs.writeFile(cbPath, dv, (err) => {
  console.log("writeFile callback DataView err:", err === null);
  fs.appendFile(cbPath, dv, (err2) => {
    console.log("appendFile callback DataView err:", err2 === null);
    console.log("callback DataView content:", fs.readFileSync(cbPath, "utf8"));
  });
});
