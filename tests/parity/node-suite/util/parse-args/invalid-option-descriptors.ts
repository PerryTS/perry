import { parseArgs } from "node:util";

function probe(label: string, options: any): void {
  try {
    parseArgs({ options });
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code", err?.message);
  }
}

probe("descriptor null", { x: null });
probe("descriptor array", { x: [] });
probe("descriptor string", { x: "bad" });
probe("type missing", { x: {} });
probe("type invalid", { x: { type: "number" } });
probe("short empty", { x: { type: "boolean", short: "" } });
probe("short long", { x: { type: "boolean", short: "ab" } });
probe("short number", { x: { type: "boolean", short: 1 } });
probe("multiple string", { x: { type: "string", multiple: "yes" } });
probe("multiple undefined", { x: { type: "boolean", multiple: undefined } });
probe("valid multiple", { x: { type: "string", multiple: true } });
