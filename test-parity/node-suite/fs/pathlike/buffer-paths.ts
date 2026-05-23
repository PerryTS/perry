import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_buffer_paths";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(Buffer.from(ROOT), { recursive: true });
const p = Buffer.from(ROOT + "/file.txt");

fs.writeFileSync(p, "buffer path");
console.log("exists buffer:", fs.existsSync(p));
console.log("read buffer path:", fs.readFileSync(p, "utf8"));
console.log("stat buffer isFile:", fs.statSync(p).isFile());
fs.appendFileSync(p, " ok");
console.log("append buffer path:", fs.readFileSync(p, "utf8"));
fs.unlinkSync(p);
console.log("exists after unlink:", fs.existsSync(p));
