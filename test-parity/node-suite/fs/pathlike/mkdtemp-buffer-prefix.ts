import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdtemp_buffer_prefix";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const prefix = Buffer.from(ROOT + "/tmp-");
const made = fs.mkdtempSync(prefix);
console.log("sync prefix:", made.indexOf(ROOT + "/tmp-") === 0);
console.log("sync exists:", fs.statSync(made).isDirectory());

await new Promise<void>((resolve) => {
  fs.mkdtemp(Buffer.from(ROOT + "/cb-"), {}, (err, dir) => {
    console.log("callback err null:", err === null);
    console.log("callback prefix:", dir.indexOf(ROOT + "/cb-") === 0);
    console.log("callback exists:", fs.statSync(dir).isDirectory());
    resolve();
  });
});
