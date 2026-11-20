import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_callback_write_append_flush_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const path = ROOT + "/callback.txt";
fs.writeFile(path, "write", { flush: true }, (err) => {
  console.log("write callback flush err:", err === null);
  fs.appendFile(path, "-append", { flush: true }, (err2) => {
    console.log("append callback flush err:", err2 === null);
    console.log("write append callback flush content:", fs.readFileSync(path, "utf8"));
  });
});
