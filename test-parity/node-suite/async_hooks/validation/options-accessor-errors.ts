import { AsyncResource, createHook } from "node:async_hooks";

const hookError = new Error("hook accessor failure");
const hookAccesses: string[] = [];
const hookOptions = Object.create(null, {
  init: {
    get() {
      hookAccesses.push("init");
      throw hookError;
    },
  },
  before: {
    get() {
      hookAccesses.push("before");
      return () => {};
    },
  },
});
let unexpectedHook: ReturnType<typeof createHook> | undefined;
try {
  unexpectedHook = createHook(hookOptions);
  console.log("hook accessor error: no-throw");
} catch (error) {
  console.log("hook accessor error:", error === hookError);
} finally {
  if (unexpectedHook) unexpectedHook.disable();
}
console.log("hook accessor stop order:", hookAccesses.join(","));

let initCount = 0;
const hook = createHook({
  init(_asyncId, type) {
    if (type === "AccessorErrorResource") initCount++;
  },
}).enable();
const resourceError = new Error("resource accessor failure");
const resourceAccesses: string[] = [];
const resourceOptions = Object.create(null, {
  triggerAsyncId: {
    get() {
      resourceAccesses.push("triggerAsyncId");
      throw resourceError;
    },
  },
  requireManualDestroy: {
    get() {
      resourceAccesses.push("requireManualDestroy");
      return true;
    },
  },
});
let unexpectedResource: AsyncResource | undefined;
try {
  unexpectedResource = new AsyncResource(
    "AccessorErrorResource",
    resourceOptions,
  );
  console.log("resource accessor error: no-throw");
} catch (error) {
  console.log("resource accessor error:", error === resourceError);
} finally {
  if (unexpectedResource) unexpectedResource.emitDestroy();
}
console.log("resource accessor stop order:", resourceAccesses.join(","));
console.log("failed resource init count:", initCount);

const valid = new AsyncResource("AccessorErrorResource");
console.log("valid resource after failure:", initCount === 1);
valid.emitDestroy();
hook.disable();
