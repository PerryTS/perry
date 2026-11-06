import {
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";
import { lookupService } from "node:dns";

const parentId = executionAsyncId();
let targetId = -1;
let targetTriggerId = -1;
let targetResource: object | undefined;
let callbackExecutionMatches = false;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "GETNAMEINFOREQWRAP" && targetId === -1) {
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
    lookupService("127.0.0.1", 80, (error, hostname, service) => {
      callbackInvoked = true;
      callbackExecutionMatches =
        executionAsyncId() === targetId &&
        executionAsyncResource() === targetResource;
      console.log(
        "lookupService hook result shape:",
        error !== null ||
          (typeof hostname === "string" && typeof service === "string"),
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
  "lookupService hook relationship:",
  callbackInvoked,
  targetId > 0,
  targetTriggerId === parentId,
  callbackExecutionMatches,
);
console.log("lookupService hook lifecycle:", events.join(">"));
