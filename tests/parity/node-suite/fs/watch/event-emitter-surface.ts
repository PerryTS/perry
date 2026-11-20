import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch_surface";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "watch");

const watcher = fs.watch(Buffer.from(p), { encoding: "buffer", persistent: false });
console.log("watch on type:", typeof watcher.on);
console.log("watch once type:", typeof watcher.once);
console.log("watch addListener type:", typeof watcher.addListener);
console.log("watch removeListener type:", typeof watcher.removeListener);
console.log("watch off type:", typeof (watcher as any).off);
watcher.on("change", () => {});
watcher.once("close", () => {});
watcher.addListener("error", () => {});
watcher.removeListener("error", () => {});
(watcher as any).off("change", () => {});
watcher.close();

const statWatcher = fs.watchFile(p, { bigint: false, interval: 10, persistent: false }, () => {});
console.log("statWatcher on type:", typeof statWatcher.on);
console.log("statWatcher close type:", typeof (statWatcher as any).close);
statWatcher.on("change", () => {});
fs.unwatchFile(p);
console.log("buffer unwatch done:", true);
