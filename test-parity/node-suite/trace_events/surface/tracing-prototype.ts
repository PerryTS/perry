import { createTracing } from "node:trace_events";

const tracing = createTracing({ categories: ["surface"] });
const prototype = Object.getPrototypeOf(tracing);

for (
  const name of ["constructor", "categories", "enabled", "enable", "disable"]
) {
  const descriptor = Object.getOwnPropertyDescriptor(prototype, name)!;
  console.log(
    name,
    "data:",
    "value" in descriptor,
    "get:",
    descriptor.get?.name ?? "none",
    "set:",
    descriptor.set?.name ?? "none",
    "writable:",
    String(descriptor.writable),
    "enumerable:",
    descriptor.enumerable,
    "configurable:",
    descriptor.configurable,
  );
}

console.log(
  "prototype parent:",
  Object.getPrototypeOf(prototype) === Object.prototype,
);
console.log(
  "instance own names:",
  Object.getOwnPropertyNames(tracing).join(","),
);
console.log(
  "instance owns constructor:",
  Object.hasOwn(tracing, "constructor"),
);
