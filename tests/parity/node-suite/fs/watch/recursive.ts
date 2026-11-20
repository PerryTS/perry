import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch_recursive";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/sub", { recursive: true });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const waitUntil = async (predicate) => {
  for (let i = 0; i < 80; i++) {
    if (predicate()) return true;
    await sleep(25);
  }
  return false;
};
const normalize = (name) => String(name).replaceAll("\\", "/");

const events = [];
const watcher = fs.watch(ROOT, { persistent: false, recursive: true }, (eventType, filename) => {
  events.push([eventType, normalize(filename)]);
});

const nested = ROOT + "/sub/nested.txt";
await sleep(80);
fs.writeFileSync(nested, "one");
const created = await waitUntil(() =>
  events.some(([type, name]) => type === "rename" && name === "sub/nested.txt")
);

events.length = 0;
fs.appendFileSync(nested, "two");
const changed = await waitUntil(() =>
  events.some(([type, name]) => (type === "change" || type === "rename") && name === "sub/nested.txt")
);

watcher.close();
console.log("watch recursive create:", created);
console.log("watch recursive update:", changed);
