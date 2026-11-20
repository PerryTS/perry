import { parseArgs } from "node:util";

function probe(label: string, options: any): void {
  try {
    const result = parseArgs({ args: [], options });
    console.log(label, "OK", JSON.stringify(result.values));
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code");
  }
}

probe("string default number", { x: { type: "string", default: 1 } });
probe("boolean default string", { x: { type: "boolean", default: "yes" } });
probe("multiple string scalar default", { x: { type: "string", multiple: true, default: "x" } });
probe("multiple string number array", { x: { type: "string", multiple: true, default: [1] } });
probe("multiple boolean valid array", { x: { type: "boolean", multiple: true, default: [false, true] } });
