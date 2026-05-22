import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_write_append_flush_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const syncPath = ROOT + "/sync.txt";
fs.writeFileSync(syncPath, "write", { flush: true });
fs.appendFileSync(syncPath, "-append", { flush: true });
console.log("write append sync flush content:", fs.readFileSync(syncPath, "utf8"));

fs.writeFileSync(syncPath, "reset", { flush: false });
fs.appendFileSync(syncPath, "-no-flush", { flush: null });
console.log("write append sync no flush content:", fs.readFileSync(syncPath, "utf8"));
