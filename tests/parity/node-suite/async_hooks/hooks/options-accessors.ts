import { AsyncResource, createHook } from "node:async_hooks";

const accesses: string[] = [];
const events: string[] = [];
let target = -1;
const prototype = {};
for (const name of ["init", "before", "after", "destroy", "promiseResolve"]) {
  Object.defineProperty(prototype, name, {
    get() {
      accesses.push(name);
      if (name === "init") {
        return (asyncId: number, type: string) => {
          if (type === "AccessorHookResource") {
            target = asyncId;
            events.push("init");
          }
        };
      }
      return (asyncId: number) => {
        if (asyncId === target) events.push(name);
      };
    },
  });
}

const hook = createHook(Object.create(prototype)).enable();
const resource = new AsyncResource("AccessorHookResource");
resource.runInAsyncScope(() => events.push("callback"));
resource.emitDestroy();
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("hook option accessor order:", accesses.join(","));
console.log("inherited hook lifecycle:", events.join(">"));
