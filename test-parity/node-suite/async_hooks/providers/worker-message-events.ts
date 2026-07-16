import { Worker } from "node:worker_threads";
import {
  AsyncLocalStorage,
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
type Entry = {
  id: number;
  type: string;
  trigger: number;
  resource: object;
  before: number;
  after: number;
  destroy: number;
};
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type !== "WORKER" && type !== "MESSAGEPORT") return;
    const entry = {
      id: asyncId,
      type,
      trigger: triggerAsyncId,
      resource,
      before: 0,
      after: 0,
      destroy: 0,
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
  destroy(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.destroy++;
  },
}).enable();
process.chdir("test-parity/node-suite/async_hooks/providers");

let workerParent = -1;
let messageResourceMapped = false;
const result = await storage.run(
  "worker-events",
  () =>
    new Promise<string>((resolve, reject) => {
      workerParent = executionAsyncId();
      const worker = new Worker("./fixtures/context-worker.cjs");
      let reply = "";
      worker.on("online", () => {
        console.log("worker online store:", storage.getStore());
      });
      worker.on("message", (message) => {
        messageResourceMapped = entries.some(
          (entry) =>
            entry.type === "MESSAGEPORT" &&
            entry.id === executionAsyncId() &&
            entry.resource === executionAsyncResource(),
        );
        console.log("worker message store:", storage.getStore(), message.phase);
        if (message.phase === "ready") {
          worker.postMessage({ value: 41 });
        } else {
          reply = String(message.value);
          worker.terminate();
        }
      });
      worker.on("error", reject);
      worker.on("exit", (code) => {
        console.log("worker exit store:", storage.getStore(), code);
        resolve(reply);
      });
    }),
);
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("worker result:", result);
console.log("worker outside:", String(storage.getStore()));
const workers = entries.filter((entry) => entry.type === "WORKER");
const ports = entries.filter((entry) => entry.type === "MESSAGEPORT");
console.log(
  "worker provider resources:",
  workers.length,
  ports.length >= 2,
  entries.length > 0 &&
    entries.every((entry) => entry.trigger === workerParent),
);
console.log("worker message resource mapped:", messageResourceMapped);
console.log(
  "worker provider lifecycles:",
  workers.length === 1 &&
    ports.length >= 2 &&
    entries.every(
      (entry) =>
        entry.before > 0 && entry.before === entry.after && entry.destroy === 1,
    ),
);
