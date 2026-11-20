import * as fs from "node:fs";
import { watch } from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_watch";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const nextMatching = async (iterator, predicate) => {
  for (let i = 0; i < 8; i++) {
    const result = await iterator.next();
    if (result.done) return result;
    if (predicate(result.value)) return result;
  }
  return { done: true, value: undefined };
};

const iterator = watch(ROOT);

const firstPending = nextMatching(iterator, (event) => event.filename === "promise.txt");
await new Promise((resolve) => setTimeout(resolve, 80));
fs.writeFileSync(ROOT + "/promise.txt", "a");
const firstResult = await firstPending;

const secondPending = nextMatching(iterator, (event) => event.filename === "promise.txt");
await new Promise((resolve) => setTimeout(resolve, 80));
fs.appendFileSync(ROOT + "/promise.txt", "b");
const secondResult = await secondPending;

const returned = await iterator.return();
const afterReturn = await iterator.next();

console.log(
  "promises watch create:",
  firstResult.done === false &&
    typeof firstResult.value.eventType === "string" &&
    firstResult.value.filename === "promise.txt",
);
console.log(
  "promises watch change:",
  secondResult.done === false &&
    typeof secondResult.value.eventType === "string" &&
    secondResult.value.filename === "promise.txt",
);
console.log("promises watch return:", returned.done === true && afterReturn.done === true);
