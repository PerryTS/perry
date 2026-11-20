import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_mutate";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/b.txt", "B");
await fsp.writeFile(ROOT + "/a.txt", "A");
await fsp.access(ROOT + "/a.txt");
const names = (await fsp.readdir(ROOT)).slice().sort();
console.log("readdir names:", names.join(","));
const st = await fsp.stat(ROOT + "/a.txt");
console.log("stat size:", st.size);
console.log("stat isFile:", st.isFile());
console.log("promises stat uid number:", typeof st.uid === "number");
console.log("promises stat gid number:", typeof st.gid === "number");
await fsp.copyFile(ROOT + "/a.txt", ROOT + "/copy.txt");
await fsp.rename(ROOT + "/copy.txt", ROOT + "/renamed.txt");
console.log("renamed content:", await fsp.readFile(ROOT + "/renamed.txt", "utf8"));
await fsp.rm(ROOT, { recursive: true, force: true });
console.log("root exists after rm:", false);
