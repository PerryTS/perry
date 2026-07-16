import {
  createHook,
  executionAsyncId,
  executionAsyncResource,
} from "node:async_hooks";
import { randomBytes } from "node:crypto";

const parentId = executionAsyncId();
let targetId = -1;
let targetTriggerId = -1;
let targetResource: object | undefined;
let callbackExecutionMatches = false;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "RANDOMBYTESREQUEST" && targetId === -1) {
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

let length = -1;
try {
  length = await new Promise<number>((resolve, reject) => {
    randomBytes(16, (error, data) => {
      callbackExecutionMatches =
        executionAsyncId() === targetId &&
        executionAsyncResource() === targetResource;
      error ? reject(error) : resolve(data.length);
    });
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
}

console.log(
  "randomBytes hook relationship:",
  length,
  targetId > 0,
  targetTriggerId === parentId,
  callbackExecutionMatches,
);
console.log("randomBytes hook lifecycle:", events.join(">"));
