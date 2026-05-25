import * as timers from "node:timers";

const events: string[] = [];
const timeout: any = timers.setTimeout(() => events.push("timeout"), 1);
const interval: any = timers.setInterval(() => events.push("interval"), 1);
const immediate: any = timers.setImmediate(() => events.push("immediate"));

timers.clearTimeout(timeout, "ignored" as any);
timers.clearInterval(interval, "ignored" as any);
timers.clearImmediate(immediate, "ignored" as any);

await new Promise<void>((resolve) => timers.setTimeout(() => resolve(), 20));
console.log("cleared with extras:", events.length);
