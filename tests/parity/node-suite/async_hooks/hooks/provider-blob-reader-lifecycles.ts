import { createHook, executionAsyncId } from "node:async_hooks";

type Entry = {
  id: number;
  trigger: number;
  resource: object;
  before: number;
  after: number;
};
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type !== "BLOBREADER") return;
    const entry = {
      id: asyncId,
      trigger: triggerAsyncId,
      resource,
      before: 0,
      after: 0,
    };
    entries.push(entry);
    byId.set(asyncId, entry);
  },
  before(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.before++;
  },
  after(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.after++;
  },
}).enable();

const parent = executionAsyncId();
const blob = new Blob(["blob-reader"]);
const [text, buffer] = await Promise.all([blob.text(), blob.arrayBuffer()]);
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("blob reader results:", text, buffer.byteLength);
console.log(
  "blob reader resources:",
  entries.length,
  entries.length === 2 && entries.every((entry) => entry.id > 0),
  entries.length === 2 &&
    new Set(entries.map((entry) => entry.resource)).size === 2,
);
console.log(
  "blob reader relationships:",
  entries.length === 2 && entries.every((entry) => entry.trigger === parent),
  entries.length === 2 &&
    entries.every((entry) => entry.before > 0 && entry.before === entry.after),
);
