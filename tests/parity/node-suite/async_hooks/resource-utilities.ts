import async_hooks, {
  AsyncLocalStorage,
  AsyncResource,
  asyncWrapProviders,
  executionAsyncResource,
} from "node:async_hooks";

function firstLine(err: any): string {
  return String(err?.message || "").split("\n")[0];
}

function check(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label + " OK:", typeof value, String(value));
  } catch (err: any) {
    console.log(label + " THROW:", err?.name, err?.code || "no-code", firstLine(err));
  }
}

console.log(
  "module own:",
  Object.prototype.hasOwnProperty.call(async_hooks, "executionAsyncResource"),
  Object.prototype.hasOwnProperty.call(async_hooks, "asyncWrapProviders"),
  Object.prototype.hasOwnProperty.call(async_hooks, "AsyncResource"),
);

console.log(
  "als static:",
  typeof (AsyncLocalStorage as any).bind,
  typeof (AsyncLocalStorage as any).snapshot,
  (AsyncLocalStorage as any).bind?.length,
  (AsyncLocalStorage as any).snapshot?.length,
);
console.log(
  "als instance:",
  typeof (new AsyncLocalStorage() as any).bind,
  typeof (new AsyncLocalStorage() as any).snapshot,
);
console.log(
  "resource static bind:",
  typeof (AsyncResource as any).bind,
  (AsyncResource as any).bind?.length,
);

const als = new AsyncLocalStorage();
let bound: any;
als.run("captured", () => {
  bound = (AsyncLocalStorage as any).bind(function (this: any, a: number, b: number) {
    console.log("als bound store:", als.getStore(), this?.tag || "no-this");
    return a + b;
  });
});
als.enterWith("outer");
console.log("als bound shape:", typeof bound, bound.length);
console.log("als bound result:", bound.call({ tag: "this" }, 2, 3));
console.log("als after bound:", als.getStore());

let snapshot: any;
als.run("snap", () => {
  snapshot = (AsyncLocalStorage as any).snapshot();
});
als.enterWith("outer2");
console.log("snapshot shape:", typeof snapshot, snapshot.length);
console.log(
  "snapshot result:",
  snapshot((a: number, b: number) => {
    console.log("snapshot store:", als.getStore());
    return a + b;
  }, 4, 5),
);
console.log("als after snapshot:", als.getStore());

const topResource = executionAsyncResource();
console.log("execution top:", typeof topResource, Object.prototype.toString.call(topResource));

const resource = new AsyncResource("ProbeResource");
resource.runInAsyncScope(() => {
  console.log(
    "execution resource same:",
    executionAsyncResource() === resource,
    typeof executionAsyncResource(),
  );
});

const staticBound = (AsyncResource as any).bind(function (this: any, value: number) {
  console.log("static bind this:", this?.tag || "no-this");
  console.log("static bind execution:", typeof executionAsyncResource());
  return value * 2;
}, "StaticProbe", { tag: "bound-this" });
console.log("static bind shape:", typeof staticBound, staticBound.length);
console.log("static bind result:", staticBound.call({ tag: "call-this" }, 7));

console.log(
  "providers:",
  typeof asyncWrapProviders,
  asyncWrapProviders.NONE,
  asyncWrapProviders.DIRHANDLE,
  asyncWrapProviders.DNSCHANNEL,
  Object.keys(asyncWrapProviders).includes("PROMISE"),
  Object.isFrozen(asyncWrapProviders),
);

check("als bind non-function", () => (AsyncLocalStorage as any).bind(1));
check("snapshot ignores arg", () => typeof (AsyncLocalStorage as any).snapshot(1));
check("resource bind non-function", () => (AsyncResource as any).bind(1));
check("resource bind bad type", () => (AsyncResource as any).bind(() => {}, 1));
