import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watch_abort";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const controller = new AbortController();
let events = 0;

const watcher = fs.watch(
  ROOT,
  { persistent: false, signal: controller.signal },
  () => {
    events++;
  },
);

controller.abort();
fs.writeFileSync(ROOT + "/after-abort.txt", "x");
await sleep(120);
watcher.close();

console.log("watch abort stops:", events === 0);
