import { createHook, executionAsyncId } from "node:async_hooks";
import { createSocket } from "node:dgram";

const parentId = executionAsyncId();
let targetId = -1;
let targetTriggerId = -1;
let resourceIsObject = false;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "UDPWRAP" && targetId === -1) {
      targetId = asyncId;
      targetTriggerId = triggerAsyncId;
      resourceIsObject = typeof resource === "object" && resource !== null;
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

const socket = createSocket("udp4");
try {
  await new Promise<void>((resolve) => socket.close(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
  try {
    socket.close();
  } catch {}
}

console.log(
  "udp socket resource:",
  targetId > 0,
  targetTriggerId === parentId,
  resourceIsObject,
);
console.log("udp socket lifecycle:", events.join(">"));
