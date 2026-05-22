import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_mkdir_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}

await fsp.mkdir(ROOT + "/a/b", { recursive: true });
console.log("promises mkdir recursive dir:", (await fsp.stat(ROOT + "/a/b")).isDirectory());

await fsp.mkdir(ROOT + "/numeric", 0o700 as any);
console.log("promises mkdir numeric mode:", ((await fsp.stat(ROOT + "/numeric")).mode & 0o777).toString(8));

await fsp.mkdir(new URL("file://" + ROOT + "/url%20dir"), { recursive: true });
console.log("promises mkdir url dir:", (await fsp.stat(ROOT + "/url dir")).isDirectory());
