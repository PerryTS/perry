import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_empty_unicode";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const empty = ROOT + "/empty.txt";
const unicode = ROOT + "/mañana-☕.txt";
fs.writeFileSync(empty, "");
fs.writeFileSync(unicode, "café");
console.log("empty string:", fs.readFileSync(empty, "utf8") === "");
console.log("empty buffer length:", fs.readFileSync(empty).length);
console.log("unicode exists:", fs.existsSync(unicode));
console.log("unicode read:", fs.readFileSync(unicode, "utf8"));
console.log("unicode entries:", fs.readdirSync(ROOT).slice().sort().join("|"));
