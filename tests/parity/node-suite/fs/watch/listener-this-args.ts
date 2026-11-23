// fs.watch / fs.watchFile listeners: `this` is the watcher, the argument
// counts and kinds per event, the members return the watcher, and a second
// watchFile on a watched path returns the same StatWatcher.
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch_listener_this";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const seen = [];

const w = fs.watch(ROOT, { persistent: false }, function (ev, name) {
  if (seen.length === 0) {
    seen.push(`change this=${this === w} args=${arguments.length} ev=${typeof ev} name=${typeof name}`);
  }
});
w.on("close", function () {
  seen.push(`close this=${this === w} args=${arguments.length}`);
});
console.log("on returns the watcher:", w.on("error", () => {}) === w);
console.log("unref returns the watcher:", w.unref() === w);
fs.writeFileSync(ROOT + "/a.txt", "x");
await sleep(300);
w.close();
await sleep(50);
for (const s of seen) console.log(s);

const F = ROOT + "/f.txt";
fs.writeFileSync(F, "1");
let fired = "";
const sw = fs.watchFile(F, { interval: 20, persistent: false }, function (curr, prev) {
  if (!fired) {
    fired = `watchFile this=${this === sw} args=${arguments.length} date=${curr.mtime instanceof Date} prev=${typeof +prev.mtime}`;
  }
});
const sw2 = fs.watchFile(F, { interval: 20, persistent: false }, () => {});
console.log("watchFile again returns the same watcher:", sw2 === sw);
await sleep(60);
fs.writeFileSync(F, "22");
await sleep(400);
fs.unwatchFile(F);
console.log(fired);
fs.rmSync(ROOT, { recursive: true, force: true });
