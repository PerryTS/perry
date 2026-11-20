import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readdir_buffer_encoding";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/a.txt", "a");
fs.writeFileSync(ROOT + "/b.txt", "b");

const entries = fs.readdirSync(Buffer.from(ROOT), { encoding: "buffer" });
console.log("sync entry isBuffer:", Buffer.isBuffer(entries[0]));
console.log("sync entries:", entries.map((x) => x.toString("utf8")).join(","));

await new Promise<void>((resolve) => {
  fs.readdir(Buffer.from(ROOT), { encoding: "buffer" }, (err, names) => {
    console.log("callback err null:", err === null);
    console.log("callback entry isBuffer:", Buffer.isBuffer(names[0]));
    console.log("callback entries:", names.map((x) => x.toString("utf8")).join(","));
    resolve();
  });
});
