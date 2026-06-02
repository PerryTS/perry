import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_access";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "ok");
let accessOK = false;
try { fs.accessSync(p, fs.constants.F_OK); accessOK = true; } catch (_e) {}
const real = fs.realpathSync(p);
console.log("access ok:", accessOK);
console.log("realpath string:", typeof real === "string");
console.log("realpath suffix:", real.endsWith("/file.txt"));
