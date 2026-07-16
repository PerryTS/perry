import { AsyncResource, createHook } from "node:async_hooks";

const observed: string[] = [];
const tracked = new Set(["NamedTask", "ExplicitType", "bound-anonymous-fn"]);
const hook = createHook({
  init(_asyncId, type) {
    if (tracked.has(type)) observed.push(type);
  },
}).enable();

function NamedTask(value: string) {
  return `named:${value}`;
}
const named = AsyncResource.bind(NamedTask);
const explicit = AsyncResource.bind(
  (value: string) => `explicit:${value}`,
  "ExplicitType",
);
const anonymous = AsyncResource.bind(
  ((value: string) => `anonymous:${value}`) as (value: string) => string,
);

console.log("static bind inferred types:", observed.join(","));
console.log("static bind results:", named("a"), explicit("b"), anonymous("c"));
console.log(
  "static bind lengths:",
  named.length,
  explicit.length,
  anonymous.length,
);
hook.disable();
