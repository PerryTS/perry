import { Readable } from "node:stream";
import { spec, tap } from "node:test/reporters";

const events = [
  { type: "test:start", data: { name: "skipped", nesting: 0 } },
  {
    type: "test:pass",
    data: { name: "skipped", nesting: 0, skip: "reason", details: { type: "test" } },
  },
  { type: "test:start", data: { name: "todo", nesting: 0 } },
  {
    type: "test:pass",
    data: { name: "todo", nesting: 0, todo: "later", details: { type: "test" } },
  },
  {
    type: "test:summary",
    data: {
      counts: { tests: 2, suites: 0, pass: 0, fail: 0, cancelled: 0, skipped: 1, todo: 1 },
      duration_ms: 1,
    },
  },
];

async function collect(name: string, reporter: any) {
  let output = "";
  for await (const chunk of reporter(Readable.from(events))) output += String(chunk);
  console.log(`${name}:`, JSON.stringify(output));
}

await collect("spec", spec);
await collect("tap", tap);
