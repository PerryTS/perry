import { AsyncResource, executionAsyncId } from "node:async_hooks";

function describeValue(value) {
  if (value === undefined) return "undefined";
  if (value === null) return "null";
  if (typeof value === "symbol") return value.toString();
  if (Array.isArray(value)) return "array";
  if (typeof value === "function") return "function";
  if (typeof value === "object") return "object";
  if (Number.isNaN(value)) return "NaN";
  return String(value);
}

function logResult(prefix, label, fn) {
  try {
    const result = fn();
    console.log(prefix, label, "ok", result === undefined ? "undefined" : String(result));
  } catch (error) {
    console.log(
      prefix,
      label,
      error.name,
      error.code || "no-code",
      String(error.message).split("\n")[0],
    );
  }
}

for (const value of [undefined, null, 0, true, {}, [], Symbol("s"), () => {}]) {
  logResult("type", describeValue(value), () => new AsyncResource(value));
}

for (const value of [null, true, "x", {}, [], Symbol("s"), 1.5, NaN, Infinity]) {
  logResult("trigger", describeValue(value), () =>
    new AsyncResource("T", { triggerAsyncId: value }),
  );
}

const defaultResource = new AsyncResource("T", { triggerAsyncId: undefined });
console.log("trigger undefined ok:", defaultResource.triggerAsyncId() === executionAsyncId());
const sentinelResource = new AsyncResource("T", { triggerAsyncId: -1 });
console.log("trigger -1:", sentinelResource.triggerAsyncId());

const resource = new AsyncResource("T");
for (const value of [undefined, null, 0, true, "x", {}, [], Symbol("s")]) {
  logResult("run", describeValue(value), () => resource.runInAsyncScope(value));
}

for (const value of [undefined, null, 0, true, "x", {}, [], Symbol("s")]) {
  logResult("bind", describeValue(value), () => resource.bind(value));
}

const receiver = { tag: "receiver" };
function valid(a, b) {
  console.log("valid this:", this === receiver, a, b);
  return "ret";
}

console.log("run valid:", resource.runInAsyncScope(valid, receiver, "a", "b"));
const bound = resource.bind(valid, receiver);
console.log("bind valid type:", typeof bound);
console.log("bind valid:", bound("c", "d"));
