import { setImmediate } from "node:timers/promises";

async function report(label: string, promise: Promise<unknown>) {
  try {
    console.log(label + ":", "resolved", await promise);
  } catch (err: any) {
    console.log(
      label + ":",
      err instanceof Error,
      err.name,
      err.code || "no-code",
      typeof err.stack,
    );
  }
}

const already = new AbortController();
already.abort();
await report("already aborted", setImmediate("late", { signal: already.signal }));

const pending = new AbortController();
const promise = setImmediate("late", { signal: pending.signal });
pending.abort();
await report("aborted after scheduling", promise);

console.log("resolved:", await setImmediate("value", { ref: true }));
