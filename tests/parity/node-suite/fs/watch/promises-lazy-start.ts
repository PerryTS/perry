import * as fs from "node:fs";
import { watch } from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_watch_lazy";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const filenameOf = (result) => result.done ? "done" : String(result.value?.filename);

const iterator = watch(ROOT);

await sleep(80);
fs.writeFileSync(ROOT + "/early.txt", "early");

const pending = iterator.next().then(filenameOf);
const prePullOutcome = await Promise.race([
  pending,
  sleep(200).then(() => "timeout"),
]);

await sleep(80);
fs.writeFileSync(ROOT + "/late.txt", "late");
const postPullOutcome = await Promise.race([
  pending,
  sleep(1000).then(() => "timeout"),
]);

await iterator.return();

console.log("promises watch pre-pull ignored:", prePullOutcome === "timeout");
console.log("promises watch post-pull delivered:", postPullOutcome === "late.txt");
