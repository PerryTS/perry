import { promiseHooks } from "node:v8";

console.log("type:", typeof promiseHooks);
console.log("keys:", Object.keys(promiseHooks).join(","));
for (
  const key of [
    "createHook",
    "onInit",
    "onBefore",
    "onAfter",
    "onSettled",
  ] as const
) {
  const descriptor = Object.getOwnPropertyDescriptor(promiseHooks, key)!;
  console.log(
    key,
    typeof promiseHooks[key],
    promiseHooks[key].length,
    descriptor.enumerable,
    descriptor.writable,
    descriptor.configurable,
  );
}

const stop = promiseHooks.createHook({});
console.log("stop:", typeof stop, stop.length, stop.name);
console.log("stop returns:", stop(), stop());
