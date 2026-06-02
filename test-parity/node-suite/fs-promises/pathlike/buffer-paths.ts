import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fsp_buffer_paths";
try { await fsp.rm(Buffer.from(ROOT), { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(Buffer.from(ROOT), { recursive: true });
const p = Buffer.from(ROOT + "/file.txt");

await fsp.writeFile(p, "promise buffer path");
console.log("read buffer path:", await fsp.readFile(p, "utf8"));
console.log("stat buffer isFile:", (await fsp.stat(p)).isFile());
await fsp.appendFile(p, " ok");
console.log("append buffer path:", fs.readFileSync(ROOT + "/file.txt", "utf8"));
await fsp.unlink(p);
console.log("exists after unlink:", fs.existsSync(ROOT + "/file.txt"));
