import { AsyncLocalStorage, AsyncResource, executionAsyncId } from "node:async_hooks";

function probe(label: string, callback: () => unknown) {
  try {
    console.log(label, "ok", callback());
  } catch (error: any) {
    console.log(label, error.name, error.code || "no-code");
  }
}
const resource = new AsyncResource("ReceiverContract");
const id = resource.asyncId;
const trigger = resource.triggerAsyncId;
const run = resource.runInAsyncScope;
const destroy = resource.emitDestroy;
const bind = resource.bind;
probe("undefined id", () => id());
probe("null id", () => id.call(null));
probe("undefined trigger", () => trigger());
probe("null trigger", () => trigger.call(null));
probe("undefined run", () => run(() => "unexpected"));
probe("null run", () => run.call(null, () => "unexpected"));
probe("undefined destroy", () => destroy());
probe("null destroy", () => destroy.call(null));
probe("undefined run invalid callback", () => run(0 as any));
probe("null run invalid callback", () => run.call(null, 0 as any));
probe("detached bind invalid callback", () => bind(0 as any));
probe("null bind invalid callback explicit this", () => bind.call(null, 0 as any, false));
const detached = bind(() => "unexpected");
const nullBound = bind.call(null, () => "unexpected");
console.log("detached bind functions", typeof detached, typeof nullBound);
probe("detached bound invocation", () => detached());
probe("null bound invocation", () => nullBound());
probe("detached bind explicit this", () => bind(() => "unexpected", false));
probe("null bind explicit this", () => bind.call(null, () => "unexpected", false));

// An unbranded receiver can still supply a real runInAsyncScope method.
const custom = {
  tag: "custom",
  runInAsyncScope(callback: any, receiver: any, value: string) {
    return `${this.tag}:${callback.call(receiver, value)}`;
  },
};
function callback(this: any, value: string) {
  return `${this.tag}:${value}`;
}
const dynamic = bind.call(custom, callback);
const explicit = bind.call(custom, callback, { tag: "explicit" });
console.log("custom dynamic", dynamic.call({ tag: "dynamic" }, "first"));
console.log("custom explicit", explicit.call({ tag: "ignored" }, "first"));
custom.runInAsyncScope = function (callback: any, receiver: any, value: string) {
  return `changed:${callback.call(receiver, value)}`;
};
console.log("custom dynamic lookup", dynamic.call({ tag: "dynamic" }, "second"));
console.log("custom explicit captured", explicit("second"));

const als = new AsyncLocalStorage();
const attached = als.run("captured", () => new AsyncResource("AttachedContract"));
const bound = attached.bind(function (this: any, value: string) {
  return `${als.getStore()}:${this.tag}:${value}:${executionAsyncId() === attached.asyncId()}`;
});
console.log("attached scope", attached.runInAsyncScope(() => executionAsyncId() === attached.asyncId()));
console.log("attached bound", bound.call({ tag: "receiver" }, "argument"));
console.log("attached restored", String(als.getStore()));
probe("attached invalid callback", () => attached.bind(0 as any));
probe("attached scope invalid callback", () => attached.runInAsyncScope(0 as any));
resource.emitDestroy();
attached.emitDestroy();
