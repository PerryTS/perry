import { parseArgs } from "node:util";

function show(label: string, value: unknown): void {
  console.log(label + ":", JSON.stringify(value));
}

const parsed = parseArgs({
  args: ["--tag", "a", "--tag=b", "--flag", "--flag", "--mode", "dev", "--mode", "prod"],
  options: {
    tag: { type: "string", multiple: true },
    flag: { type: "boolean", multiple: true },
    color: { type: "string", default: "auto" },
    verbose: { type: "boolean", default: true },
    list: { type: "string", multiple: true, default: ["x"] },
    bools: { type: "boolean", multiple: true, default: [false] },
    mode: { type: "string", default: "auto" },
  },
});

show("values", parsed.values);
show("array flags", {
  tag: Array.isArray(parsed.values.tag),
  flag: Array.isArray(parsed.values.flag),
  list: Array.isArray(parsed.values.list),
  bools: Array.isArray(parsed.values.bools),
});
