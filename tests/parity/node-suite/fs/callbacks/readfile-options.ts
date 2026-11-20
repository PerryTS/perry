import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_cb_readfile_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "callback options");

await new Promise<void>((resolve) => {
  fs.readFile(p, { encoding: "utf8", flag: "r" }, (err, data) => {
    console.log("object err null:", err === null);
    console.log("object data:", data);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.readFile(p, { encoding: null, flag: "r" }, (err, data) => {
    console.log("null err null:", err === null);
    console.log("null isBuffer:", Buffer.isBuffer(data));
    console.log("null text:", data.toString("utf8"));
    resolve();
  });
});
