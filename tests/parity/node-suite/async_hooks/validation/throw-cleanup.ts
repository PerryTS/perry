import {
  AsyncLocalStorage,
  AsyncResource,
  executionAsyncId,
} from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
storage.enterWith("outer");

try {
  storage.run("inner", () => {
    console.log("run before throw:", storage.getStore());
    throw new Error("run-error");
  });
} catch (error) {
  console.log("run error:", (error as Error).message);
}
console.log("run cleanup:", storage.getStore());

try {
  storage.exit(() => {
    console.log("exit before throw:", String(storage.getStore()));
    throw new Error("exit-error");
  });
} catch (error) {
  console.log("exit error:", (error as Error).message);
}
console.log("exit cleanup:", storage.getStore());

const parentId = executionAsyncId();
const resource = new AsyncResource("ParityThrowCleanup");
try {
  resource.runInAsyncScope(() => {
    console.log(
      "resource before throw:",
      executionAsyncId() === resource.asyncId(),
    );
    throw new Error("resource-error");
  });
} catch (error) {
  console.log("resource error:", (error as Error).message);
}
console.log("resource cleanup:", executionAsyncId() === parentId);

const bound = AsyncLocalStorage.bind(() => {
  console.log("bound before throw:", storage.getStore());
  throw new Error("bound-error");
});
storage.enterWith("current");
try {
  bound();
} catch (error) {
  console.log("bound error:", (error as Error).message);
}
console.log("bound cleanup:", storage.getStore());

resource.emitDestroy();
storage.disable();
