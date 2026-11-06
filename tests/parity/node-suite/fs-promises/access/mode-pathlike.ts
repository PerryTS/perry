import * as fsp from "node:fs/promises";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_promises_access_mode_pathlike";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const file = ROOT + "/script.sh";
await fsp.writeFile(file, "#!/bin/sh\nexit 0\n");
await fsp.chmod(file, 0o755);

await fsp.access(Buffer.from(file), 4 | 1);
console.log("promises access buffer rx ok:", true);
await fsp.access(pathToFileURL(file), 2);
console.log("promises access url writable:", true);
