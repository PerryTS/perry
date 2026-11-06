import asyncHooksDefault, * as async_hooks from "node:async_hooks";
import {
  AsyncLocalStorage,
  AsyncResource,
  asyncWrapProviders,
  executionAsyncResource,
} from "node:async_hooks";

function summarizeError(error: any): string {
  return `${error.name} ${error.code || "no-code"}`;
}

function probe(label: string, fn: () => any) {
  try {
    const value = fn();
    console.log(label, "ok", value === undefined ? "undefined" : String(value));
  } catch (error: any) {
    console.log(label, summarizeError(error));
  }
}

const als = new AsyncLocalStorage();

console.log(
  "static helper typeofs:",
  typeof AsyncLocalStorage.bind,
  typeof AsyncLocalStorage.snapshot,
  typeof AsyncResource.bind,
  typeof executionAsyncResource,
  typeof async_hooks.executionAsyncResource,
);
console.log("instance helper absence:", typeof (als as any).bind, typeof (als as any).snapshot);
console.log(
  "provider sample:",
  typeof asyncWrapProviders,
  asyncWrapProviders.NONE,
  asyncWrapProviders.DIRHANDLE,
  asyncWrapProviders.PROMISE,
  Object.keys(asyncWrapProviders).slice(0, 3).join("|"),
);

const boundFromAls = als.run("ctx", () =>
  AsyncLocalStorage.bind((a: number, b: number) => `${als.getStore()}:${a + b}`),
);
console.log("als bind:", typeof boundFromAls, boundFromAls(2, 3), String(als.getStore()));

const snapshotRunner = als.run("snap", () => AsyncLocalStorage.snapshot());
console.log(
  "als snapshot:",
  typeof snapshotRunner,
  snapshotRunner((a: string, b: string) => `${als.getStore()}:${a}:${b}`, "x", "y"),
  String(als.getStore()),
);

probe("als bind bad", () => AsyncLocalStorage.bind(0 as any));
probe("als snapshot bad", () => snapshotRunner(0 as any));

const staticBoundResource = AsyncResource.bind(
  function (this: any, value: number) {
    return `${this && this.tag}:${value}`;
  },
  "StaticProbe",
);
console.log(
  "resource static bind:",
  typeof staticBoundResource,
  staticBoundResource.call({ tag: "recv" }, 7),
);
probe("resource static bind bad", () => AsyncResource.bind(0 as any));

const resource = new AsyncResource("ScopeProbe");
console.log(
  "execution resource objects:",
  typeof executionAsyncResource(),
  typeof resource.runInAsyncScope(() => executionAsyncResource()),
  typeof async_hooks.executionAsyncResource(),
);

const capturedExecutionAsyncResource = async_hooks.executionAsyncResource;
console.log(
  "captured executionAsyncResource:",
  typeof capturedExecutionAsyncResource,
  typeof capturedExecutionAsyncResource(),
);

console.log(
  "default helpers:",
  typeof asyncHooksDefault.executionAsyncResource,
  typeof asyncHooksDefault.asyncWrapProviders,
  asyncHooksDefault.asyncWrapProviders.NONE,
);
