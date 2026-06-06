import * as fs from "node:fs";
import { watch } from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_watch_recursive";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/sub", { recursive: true });

const normalize = (name) => String(name).replaceAll("\\", "/");
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const nextMatching = async (iterator, predicate) => {
  for (let i = 0; i < 8; i++) {
    const result = await iterator.next();
    if (result.done) return result;
    if (predicate(result.value)) return result;
  }
  return { done: true, value: undefined };
};

const recursive = watch(ROOT, { recursive: true });
const recursivePending = nextMatching(
  recursive,
  (event) => normalize(event.filename) === "sub/nested.txt",
);
await new Promise((resolve) => setTimeout(resolve, 80));
fs.writeFileSync(ROOT + "/sub/nested.txt", "x");
const recursiveResult = await recursivePending;
await recursive.return();

const abortRoot = ROOT + "/abort";
fs.mkdirSync(abortRoot);
const controller = new AbortController();
const aborted = watch(abortRoot, { signal: controller.signal });
const pending = aborted.next();
controller.abort();

let pendingRejected = false;
let futureOutcome = "pending";
try {
  await pending;
} catch (err) {
  pendingRejected = err && err.name === "AbortError";
}
futureOutcome = await Promise.race([
  aborted.next().then(
    (result) => (result.done === true ? "done" : "value"),
    (err) => (err && err.name === "AbortError" ? "abort" : "error"),
  ),
  sleep(200).then(() => "timeout"),
]);
await Promise.race([aborted.return(), sleep(200)]);

console.log(
  "promises watch recursive:",
  recursiveResult.done === false &&
    typeof recursiveResult.value.eventType === "string" &&
    normalize(recursiveResult.value.filename) === "sub/nested.txt",
);
console.log("promises watch abort pending:", pendingRejected);
console.log(
  "promises watch abort future:",
  futureOutcome === "done" || futureOutcome === "abort" || futureOutcome === "timeout",
);
