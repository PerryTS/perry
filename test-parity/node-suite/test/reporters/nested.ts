import { Readable } from "node:stream";
import { spec, tap } from "node:test/reporters";

const events = [
  { type: "test:start", data: { name: "parent", nesting: 0 } },
  { type: "test:start", data: { name: "child", nesting: 1 } },
  {
    type: "test:pass",
    data: { name: "child", nesting: 1, duration_ms: 1, details: { type: "test" } },
  },
  {
    type: "test:pass",
    data: { name: "parent", nesting: 0, duration_ms: 2, details: { type: "suite" } },
  },
];

async function collect(name: string, reporter: any) {
  let output = "";
  for await (const chunk of reporter(Readable.from(events))) output += String(chunk);
  console.log(`${name}:`, JSON.stringify(output));
}

await collect("spec", spec);
await collect("tap", tap);
