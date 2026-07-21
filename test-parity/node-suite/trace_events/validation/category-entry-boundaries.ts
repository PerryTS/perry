import { createTracing } from "node:trace_events";

function probe(label: string, value: any) {
  try {
    createTracing({ categories: ["valid", value] });
    console.log(label, "OK");
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code);
  }
}

probe("undefined", undefined);
probe("null", null);
probe("boolean", true);
probe("number", 1);
probe("object", {});
probe("array", []);
probe("function", () => "category");

const sparse = new Array(1);
probe("sparse", sparse[0]);
