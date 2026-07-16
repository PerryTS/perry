import { createHook, executionAsyncId } from "node:async_hooks";
import { pbkdf2 } from "node:crypto";

let requestId = -1;
let triggerId = -1;
let beforeCount = 0;
let afterCount = 0;
let destroyCount = 0;
const parentId = executionAsyncId();
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "PBKDF2REQUEST") {
      requestId = asyncId;
      triggerId = triggerAsyncId;
    }
  },
  before(asyncId) {
    if (asyncId === requestId) beforeCount++;
  },
  after(asyncId) {
    if (asyncId === requestId) afterCount++;
  },
  destroy(asyncId) {
    if (asyncId === requestId) destroyCount++;
  },
}).enable();

const keyLength = await new Promise<number>((resolve, reject) => {
  pbkdf2("password", "salt", 1, 20, "sha256", (error, key) => {
    if (error) return reject(error);
    console.log(
      "pbkdf2 callback lifecycle:",
      requestId > 0,
      triggerId === parentId,
      beforeCount,
    );
    resolve(key.length);
  });
});

await new Promise<void>((resolve) => setImmediate(resolve));
console.log(
  "pbkdf2 completion lifecycle:",
  keyLength,
  afterCount,
  destroyCount,
);
hook.disable();
