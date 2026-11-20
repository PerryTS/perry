import { createHook, executionAsyncId } from "node:async_hooks";

type Activity = {
  id: number;
  trigger: number;
  events: string[];
  resource: object;
};
const activities: Activity[] = [];
const byId = new Map<number, Activity>();
const root = executionAsyncId();
let accepting = true;
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (accepting && type === "Timeout") {
      const activity = {
        id: asyncId,
        trigger: triggerAsyncId,
        events: ["init"],
        resource,
      };
      activities.push(activity);
      byId.set(asyncId, activity);
    }
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

const handles: object[] = [];
await new Promise<void>((resolve) => {
  handles.push(
    setTimeout(() => {
      handles.push(
        setTimeout(() => {
          handles.push(
            setTimeout(() => {
              accepting = false;
              resolve();
            }, 0),
          );
        }, 0),
      );
    }, 0),
  );
});
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log("timeout count:", activities.length);
console.log(
  "timeout resources:",
  activities.length === 3 &&
    activities.every((a, i) => a.resource === handles[i]),
);
console.log(
  "timeout trigger chain:",
  activities[0]?.trigger === root,
  activities[1]?.trigger === activities[0]?.id,
  activities[2]?.trigger === activities[1]?.id,
);
console.log(
  "timeout lifecycles:",
  activities.map((a) => a.events.join(">")).join("|"),
);
