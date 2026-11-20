import * as traceEvents from "node:trace_events";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "OK", typeof value);
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

probe("missing options", () => traceEvents.createTracing());
probe("null options", () => traceEvents.createTracing(null as any));
probe("array options", () => traceEvents.createTracing([] as any));
probe("missing categories", () => traceEvents.createTracing({} as any));
probe("string categories", () => traceEvents.createTracing({ categories: "node" as any }));
probe("empty categories", () => traceEvents.createTracing({ categories: [] }));
probe("number element", () => traceEvents.createTracing({ categories: ["node", 1 as any] }));
probe("extra options", () => traceEvents.createTracing({ categories: ["node"], other: 1 } as any));
