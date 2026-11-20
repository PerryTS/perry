import {
  AsyncResource,
  createHook,
  executionAsyncResource,
} from "node:async_hooks";

const context = Symbol("context");
type ContextResource = object & { [context]?: string };
const hook = createHook({
  init(_asyncId, _type, _triggerAsyncId, resource) {
    const current = executionAsyncResource() as ContextResource;
    (resource as ContextResource)[context] = current[context];
  },
}).enable();

async function exercise(label: string) {
  await new Promise<void>((resolve) => setImmediate(resolve));
  await Promise.resolve();
  return (executionAsyncResource() as ContextResource)[context];
}

async function start(label: string) {
  const resource = new AsyncResource(`Property-${label}`) as AsyncResource &
    ContextResource;
  resource[context] = label;
  const promise = resource.runInAsyncScope(() => exercise(label));
  try {
    return await promise;
  } finally {
    resource.emitDestroy();
  }
}

const values = await Promise.all([start("first"), start("second")]);
console.log("execution resource propagated values:", values.join(","));
hook.disable();
