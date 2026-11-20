import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_watchfile_delivery";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
fs.writeFileSync(file, "a");

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const waitUntil = async (predicate) => {
  for (let i = 0; i < 80; i++) {
    if (predicate()) return true;
    await sleep(25);
  }
  return false;
};

let a = 0;
let b = 0;
let grew = false;
const listenerA = (curr, prev) => {
  a++;
  grew ||= curr.size > prev.size;
};
const listenerB = () => {
  b++;
};

fs.watchFile(file, { interval: 20, persistent: false }, listenerA);
fs.watchFile(file, { interval: 20, persistent: false }, listenerB);

fs.appendFileSync(file, "b");
const first = await waitUntil(() => a > 0 && b > 0 && grew);
await sleep(200);

fs.unwatchFile(file, listenerA);
await sleep(100);
const aBefore = a;
const bBefore = b;
fs.appendFileSync(file, "c");
const second = await waitUntil(() => b > bBefore);
await sleep(200);

fs.unwatchFile(file);
await sleep(100);
const bBeforeAll = b;
fs.appendFileSync(file, "d");
await sleep(200);
fs.unwatchFile(file);

console.log("watchFile both listeners:", first);
console.log("watchFile remove one:", second && a === aBefore && b > bBefore);
console.log("watchFile remove all:", b === bBeforeAll);
console.log("watchFile unwatch noop:", true);
