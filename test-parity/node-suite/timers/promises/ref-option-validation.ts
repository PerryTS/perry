import * as timers from "node:timers/promises";

async function probe(label: string, fn: () => Promise<unknown>) {
  try {
    await fn();
    console.log(label, "OK");
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

await probe("setTimeout ref string", () =>
  timers.setTimeout(1, undefined, { ref: "no" as any })
);
await probe("setImmediate ref number", () =>
  timers.setImmediate(undefined, { ref: 0 as any })
);
await probe("setInterval ref object", () =>
  timers.setInterval(1, undefined, { ref: {} as any }).next()
);
await probe("scheduler.wait ref string", () =>
  timers.scheduler.wait(1, { ref: "no" as any })
);
