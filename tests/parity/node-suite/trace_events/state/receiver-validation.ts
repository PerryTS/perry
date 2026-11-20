import { createTracing } from "node:trace_events";

const tracing = createTracing({ categories: ["receiver"] });
const prototype = Object.getPrototypeOf(tracing);
const categories = Object.getOwnPropertyDescriptor(prototype, "categories")!
  .get!;
const enabled = Object.getOwnPropertyDescriptor(prototype, "enabled")!.get!;

function probe(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, "THROW", error.name, String(error.code));
  }
}

probe("enable null", () => tracing.enable.call(null));
probe("enable object", () => tracing.enable.call({}));
probe("disable prototype", () => tracing.disable.call(prototype));
probe("categories object", () => categories.call({}));
probe("enabled null", () => enabled.call(null));
