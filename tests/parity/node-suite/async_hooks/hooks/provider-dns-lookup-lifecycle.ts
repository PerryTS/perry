import {
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";
import { lookup } from "node:dns";

const parentId = executionAsyncId();
let targetId = -1;
let targetTriggerId = -1;
let targetResource: object | undefined;
let callbackExecutionMatches = false;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "GETADDRINFOREQWRAP" && targetId === -1) {
      targetId = asyncId;
      targetTriggerId = triggerAsyncId;
      targetResource = resource;
      events.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === targetId) events.push("before");
  },
  after(asyncId) {
    if (asyncId === targetId) events.push("after");
  },
  destroy(asyncId) {
    if (asyncId === targetId) events.push("destroy");
  },
}).enable();

let callbackInvoked = false;
try {
  await new Promise<void>((resolve) => {
    lookup("localhost", (error, address) => {
      callbackInvoked = true;
      callbackExecutionMatches =
        executionAsyncId() === targetId &&
        executionAsyncResource() === targetResource;
      console.log(
        "dns lookup hook result shape:",
        error !== null || typeof address === "string",
      );
      resolve();
    });
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
}

console.log(
  "dns lookup hook relationship:",
  callbackInvoked,
  targetId > 0,
  targetTriggerId === parentId,
  callbackExecutionMatches,
);
console.log("dns lookup hook lifecycle:", events.join(">"));
