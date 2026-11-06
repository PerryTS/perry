import { createHook, executionAsyncId } from "node:async_hooks";
import { deflate } from "node:zlib";

const parentId = executionAsyncId();
let targetId = -1;
let targetTriggerId = -1;
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "ZLIB" && targetId === -1) {
      targetId = asyncId;
      targetTriggerId = triggerAsyncId;
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
    deflate("zlib-hook-payload", (error, data) => {
      error ? reject(error) : resolve(data.length);
    });
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
}

console.log(
  "zlib hook relationship:",
  length > 0,
  targetId > 0,
  targetTriggerId === parentId,
);
console.log("zlib hook lifecycle:", events.join(">"));
