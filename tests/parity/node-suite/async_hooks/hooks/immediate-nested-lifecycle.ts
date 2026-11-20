import { createHook, executionAsyncId } from "node:async_hooks";

const parentId = executionAsyncId();
type Entry = {
  asyncId: number;
  triggerAsyncId: number;
  resource: object;
  events: string[];
};
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type !== "Immediate" || entries.length >= 2) return;
    const entry = { asyncId, triggerAsyncId, resource, events: ["init"] };
    entries.push(entry);
    byId.set(asyncId, entry);
  },
  before(asyncId) {
    byId.get(asyncId)?.events.push("before");
  },
  after(asyncId) {
    byId.get(asyncId)?.events.push("after");
  },
  destroy(asyncId) {
    byId.get(asyncId)?.events.push("destroy");
  },
}).enable();

let firstHandle: ReturnType<typeof setImmediate> | undefined;
let secondHandle: ReturnType<typeof setImmediate> | undefined;
await new Promise<void>((resolve) => {
  firstHandle = setImmediate(() => {
    secondHandle = setImmediate(resolve);
  });
});
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("nested immediate count:", entries.length);
console.log(
  "nested immediate resources:",
  entries.length === 2 && entries[0].resource === firstHandle,
  entries.length === 2 && entries[1].resource === secondHandle,
);
console.log(
  "nested immediate triggers:",
  entries.length === 2 && entries[0].triggerAsyncId === parentId,
  entries.length === 2 && entries[1].triggerAsyncId === entries[0].asyncId,
);
console.log(
  "nested immediate lifecycles:",
  entries.map((entry) => entry.events.join(">")).join(","),
);
