import { AsyncResource } from "node:async_hooks";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "ok", typeof value);
  } catch (error: any) {
    console.log(label, error.name, error.code || "no-code");
  }
}
const resource = new AsyncResource("ReceiverValidation");
const asyncId = resource.asyncId;
const triggerAsyncId = resource.triggerAsyncId;
const runInAsyncScope = resource.runInAsyncScope;
const emitDestroy = resource.emitDestroy;
const bind = resource.bind;
probe("detached asyncId", () => asyncId());
probe("detached triggerAsyncId", () => triggerAsyncId());
probe("detached runInAsyncScope", () => runInAsyncScope(() => "result"));
probe("detached emitDestroy", () => emitDestroy());
probe("detached bind", () => bind(() => "result"));
resource.emitDestroy();
