import { setTimeout as delay, scheduler } from "node:timers/promises";

async function shape(label: string, promise: Promise<unknown>) {
  try {
    await promise;
  } catch (err: any) {
    console.log(label + ":", err instanceof Error, err.name, err.code || "no-code", typeof err.stack, String(err.stack).includes("AbortError"));
  }
}

const timeoutAc = new AbortController();
const timeoutPromise = delay(50, "late", { signal: timeoutAc.signal });
timeoutAc.abort();
await shape("timeout", timeoutPromise);

const waitAc = new AbortController();
const waitPromise = scheduler.wait(50, { signal: waitAc.signal });
waitAc.abort();
await shape("wait", waitPromise);
