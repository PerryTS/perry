import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readlink_encoding_pathlike";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/target [] á.txt", "x");
fs.symlinkSync("target [] á.txt", ROOT + "/link [] á.txt");

const syncTarget = fs.readlinkSync(Buffer.from(ROOT + "/link [] á.txt"));
console.log("readlinkSync buffer path:", syncTarget);

const syncBuffer = fs.readlinkSync(Buffer.from(ROOT + "/link [] á.txt"), { encoding: "buffer" });
console.log("readlinkSync buffer encoding is buffer:", Buffer.isBuffer(syncBuffer));
console.log("readlinkSync buffer encoding value:", syncBuffer.toString());

fs.readlink(Buffer.from(ROOT + "/link [] á.txt"), { encoding: "buffer" }, (err, data) => {
  console.log("readlink callback buffer err:", err === null);
  console.log("readlink callback buffer is buffer:", Buffer.isBuffer(data));
  console.log("readlink callback buffer value:", data.toString());
});
