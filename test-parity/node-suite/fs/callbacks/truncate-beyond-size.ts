import * as fs from "node:fs";

// Truncating beyond the current file size must zero-extend the file
// (matching POSIX `ftruncate` semantics that Node exposes).
const ROOT = "/tmp/perry_node_suite_fs_truncate_beyond";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const path = ROOT + "/file.bin";
fs.writeFileSync(path, "abc");

fs.truncateSync(path, 8);
const after = fs.readFileSync(path);
console.log("truncate beyond size length:", after.length);
console.log("truncate beyond size head bytes:", after[0], after[1], after[2]);
console.log("truncate beyond size tail zeros:", after[3], after[4], after[5], after[6], after[7]);
