import { Readable } from "node:stream";
import { dot, junit, lcov, spec, tap } from "node:test/reporters";

const events = [
  { type: "test:start", data: { name: "alpha", nesting: 0 } },
  {
    type: "test:pass",
    data: {
      name: "alpha",
      nesting: 0,
      duration_ms: 1.23,
      details: { type: "test" },
    },
  },
  { type: "test:diagnostic", data: { message: "hello <world>", nesting: 0 } },
  {
    type: "test:summary",
    data: {
      counts: {
        tests: 1,
        suites: 0,
        pass: 1,
        fail: 0,
        cancelled: 0,
        skipped: 0,
        todo: 0,
      },
      duration_ms: 4.56,
    },
  },
];

async function collect(name: string, reporter: any): Promise<void> {
  let output = "";
  const result = reporter(Readable.from(events));
  if (typeof result.write === "function") {
    const transform = reporter();
    transform.on("data", (chunk: unknown) => {
      output += String(chunk);
    });
    await new Promise<void>((resolve) => {
      transform.on("end", resolve);
      Readable.from(events).pipe(transform);
    });
  } else {
    for await (const chunk of result) {
      output += String(chunk);
    }
  }
  console.log(`${name}:`, JSON.stringify(output));
}

await collect("spec", spec);
await collect("tap", tap);
await collect("dot", dot);
await collect("junit", junit);
await collect("lcov", lcov);
