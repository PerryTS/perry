import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "watch");

const watcher = fs.watch(p, () => {});
console.log("watch close type:", typeof watcher.close);
console.log("watch ref type:", typeof watcher.ref);
console.log("watch unref type:", typeof watcher.unref);
watcher.unref();
watcher.ref();
watcher.close();

const statWatcher = fs.watchFile(p, { interval: 10 }, () => {});
console.log("watchFile ref type:", typeof statWatcher.ref);
console.log("watchFile unref type:", typeof statWatcher.unref);
statWatcher.unref();
statWatcher.ref();
fs.unwatchFile(p);
console.log("unwatchFile done:", true);
