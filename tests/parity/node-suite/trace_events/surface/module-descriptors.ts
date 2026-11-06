import * as traceEvents from "node:trace_events";

function descriptor(name: "createTracing" | "getEnabledCategories") {
  const value = Object.getOwnPropertyDescriptor(traceEvents, name)!;
  console.log(
    name,
    typeof value.value,
    value.value.name,
    value.value.length,
    value.enumerable,
    value.configurable,
    value.writable,
  );
}

console.log(
  "own names:",
  Object.getOwnPropertyNames(traceEvents).sort().join(","),
);
descriptor("createTracing");
descriptor("getEnabledCategories");
console.log("frozen:", Object.isFrozen(traceEvents));
