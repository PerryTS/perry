import { createTracing } from "node:trace_events";

const tracing = createTracing({ categories: ["readonly"] });

function assign(label: string, name: "categories" | "enabled", value: any) {
  try {
    (tracing as any)[name] = value;
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, "THROW", error.name, String(error.code));
  }
}

assign("categories", "categories", "changed");
assign("enabled", "enabled", true);
console.log("values:", tracing.categories, tracing.enabled);
