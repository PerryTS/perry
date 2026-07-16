import { createTracing } from "node:trace_events";

function probe(label: string, options: any) {
  try {
    const tracing = createTracing(options);
    console.log(label, "OK", tracing.categories);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code);
  }
}

probe("undefined", undefined);
probe("null", null);
probe("false", false);
probe("zero", 0);
probe("string", "category");
probe("function", () => {});
probe("array", []);
probe("date", new Date(0));
probe("null prototype", Object.create(null));
probe("extra property", { categories: ["ok"], extra: true });
