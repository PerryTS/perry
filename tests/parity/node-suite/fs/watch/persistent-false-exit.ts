import * as fs from "node:fs";
import { watch as promisesWatch } from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_watch_persistent_false";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
fs.writeFileSync(file, "watch");

fs.watch(ROOT, { persistent: false }, () => {});
fs.watchFile(file, { persistent: false, interval: 20 }, () => {});
promisesWatch(ROOT, { persistent: false });

console.log("watch persistent false exits:", true);
