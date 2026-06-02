import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-promise-resolution-extra");
const events: string[] = [];
ch.subscribe({
  start: () => events.push("start"),
  asyncStart: (ctx: any) => events.push("asyncStart:" + ctx.result),
  asyncEnd: (ctx: any) => events.push("asyncEnd:" + ctx.result),
  end: () => events.push("end"),
});
const result = await ch.tracePromise(async () => "ok", {});
console.log("result:", result);
console.log("events:", events.join("|"));
