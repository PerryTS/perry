import { createHook, executionAsyncId } from "node:async_hooks";

type Activity = { id: number; trigger: number; events: string[] };
const root = executionAsyncId();
const activities: Activity[] = [];
const byId = new Map<number, Activity>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type !== "SIGNALWRAP") return;
    const activity = { id: asyncId, trigger: triggerAsyncId, events: ["init"] };
    activities.push(activity);
    byId.set(asyncId, activity);
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
const signal = process.platform === "win32" ? "SIGTERM" : "SIGUSR2";
const listener = () => {};
process.on(signal, listener);
process.removeListener(signal, listener);
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log("signal resources:", activities.length);
console.log(
  "signal root trigger:",
  activities.length === 1 && activities[0]?.trigger === root,
);
console.log(
  "signal lifecycle:",
  activities.map((a) => a.events.join(">")).join("|"),
);
