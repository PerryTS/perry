import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_readfile_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "read options");

const text = fs.readFileSync(p, { encoding: "utf8", flag: "r" });
console.log("object encoding:", text);

const bufferFromNull = fs.readFileSync(p, { encoding: null, flag: "r" });
console.log("object null isBuffer:", Buffer.isBuffer(bufferFromNull));
console.log("object null text:", bufferFromNull.toString("utf8"));

const bufferFromFlagOnly = fs.readFileSync(p, { flag: "r" });
console.log("flag only isBuffer:", Buffer.isBuffer(bufferFromFlagOnly));
console.log("flag only text:", bufferFromFlagOnly.toString("utf8"));

const trunc = fs.readFileSync(p, { encoding: "utf8", flag: "w+" });
console.log("w+ trunc read length:", trunc.length);
console.log("w+ size:", fs.statSync(p).size);
