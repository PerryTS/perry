import { createTracing } from "node:trace_events";

function report(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ":", "OK");
  } catch (err: any) {
    console.log(label + ":", err.name, err.code, err.message.split("\n")[0]);
  }
}

report("missing options", () => createTracing(undefined as any));
report("null options", () => createTracing(null as any));
report("string options", () => createTracing("node" as any));
report("missing categories", () => createTracing({} as any));
report("non-array categories", () => createTracing({ categories: "node" } as any));
report("empty categories", () => createTracing({ categories: [] }));
report("non-string category", () => createTracing({ categories: ["node", 1] as any }));
report("extra option", () => createTracing({ categories: ["node"], ignored: true } as any));
