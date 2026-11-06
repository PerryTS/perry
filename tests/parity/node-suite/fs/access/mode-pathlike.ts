import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_access_mode_pathlike";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/script.sh";
fs.writeFileSync(file, "#!/bin/sh\nexit 0\n");
fs.chmodSync(file, 0o755);

let syncOK = false;
try { fs.accessSync(Buffer.from(file), fs.constants.R_OK | fs.constants.X_OK); syncOK = true; } catch (_e) {}
console.log("accessSync buffer rx ok:", syncOK);

fs.access(pathToFileURL(file), fs.constants.W_OK, (err) => {
  console.log("access callback url writable:", err === null);
});
