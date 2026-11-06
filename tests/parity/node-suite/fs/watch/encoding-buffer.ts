import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch_buffer";
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

let seenBuffer = false;
let seenName = "";
const watcher = fs.watch(ROOT, { encoding: "buffer", persistent: false }, (_eventType, filename) => {
  seenBuffer = Buffer.isBuffer(filename);
  seenName = Buffer.isBuffer(filename) ? filename.toString("utf8") : String(filename);
});

await sleep(80);
fs.writeFileSync(ROOT + "/buffered.txt", "x");
const delivered = await waitUntil(() => seenName === "buffered.txt");
watcher.close();

console.log("watch buffer delivered:", delivered);
console.log("watch buffer filename:", seenBuffer && seenName === "buffered.txt");
