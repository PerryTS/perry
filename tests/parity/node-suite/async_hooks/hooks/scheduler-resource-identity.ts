import { createHook, executionAsyncResource } from "node:async_hooks";

type ResourceInfo = {
  asyncId: number;
  triggerAsyncId: number;
  resource: object;
};
const resources = new Map<string, ResourceInfo>();
const executionMatches = new Map<string, boolean>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (
      (type === "Timeout" ||
        type === "Immediate" ||
        type === "Microtask" ||
        type === "TickObject") &&
      !resources.has(type)
    ) {
      resources.set(type, { asyncId, triggerAsyncId, resource });
    }
  },
}).enable();

const timeout = setTimeout(() => {
  executionMatches.set(
    "Timeout",
    executionAsyncResource() === resources.get("Timeout")?.resource,
  );
  const immediate = setImmediate(() => {
    executionMatches.set(
      "Immediate",
      executionAsyncResource() === resources.get("Immediate")?.resource,
    );
    queueMicrotask(() => {
      executionMatches.set(
        "Microtask",
        executionAsyncResource() === resources.get("Microtask")?.resource,
      );
      process.nextTick(() => {
        executionMatches.set(
          "TickObject",
          executionAsyncResource() === resources.get("TickObject")?.resource,
        );
        setImmediate(() => {
          hook.disable();
          const timeoutInfo = resources.get("Timeout");
          const immediateInfo = resources.get("Immediate");
          const microtaskInfo = resources.get("Microtask");
          const tickInfo = resources.get("TickObject");
          console.log(
            "scheduler handles match resources:",
            timeoutInfo?.resource === timeout,
            immediateInfo?.resource === immediate,
          );
          console.log(
            "scheduler execution resources:",
            executionMatches.get("Timeout"),
            executionMatches.get("Immediate"),
            executionMatches.get("Microtask"),
            executionMatches.get("TickObject"),
          );
          console.log(
            "scheduler trigger chain:",
            !!immediateInfo &&
              !!timeoutInfo &&
              immediateInfo.triggerAsyncId === timeoutInfo.asyncId,
            !!microtaskInfo &&
              !!immediateInfo &&
              microtaskInfo.triggerAsyncId === immediateInfo.asyncId,
            !!tickInfo &&
              !!microtaskInfo &&
              tickInfo.triggerAsyncId === microtaskInfo.asyncId,
          );
        });
      });
    });
  });
}, 0);
