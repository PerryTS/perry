import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch_delivery";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const waitUntil = async (predicate) => {
  for (let i = 0; i < 80; i++) {
    if (predicate()) return true;
    await sleep(25);
  }
  return false;
};

const events = [];
const watcher = fs.watch(ROOT, { persistent: false }, (eventType, filename) => {
  events.push([eventType, String(filename)]);
});

const file = ROOT + "/file.txt";
fs.writeFileSync(file, "a");
const created = await waitUntil(() =>
  events.some(([type, name]) => type === "rename" && name === "file.txt")
);

const fileEvents = [];
const fileWatcher = fs.watch(file, { persistent: false }, (eventType, filename) => {
  fileEvents.push([eventType, String(filename)]);
});
await sleep(80);
fs.appendFileSync(file, "b");
const changed = await waitUntil(() =>
  fileEvents.some(([type, name]) => type === "change" && name === "file.txt")
);

fileWatcher.close();
fileEvents.length = 0;
fs.appendFileSync(file, "after-close");
await sleep(100);

events.length = 0;
fs.unlinkSync(file);
const removed = await waitUntil(() =>
  events.some(([type, name]) => type === "rename" && name === "file.txt")
);

watcher.close();

console.log("watch create event:", created);
console.log("watch change event:", changed);
console.log("watch remove event:", removed);
console.log("watch close stops:", fileEvents.length === 0);
