import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-sync-error-order");
const events: string[] = [];
ch.subscribe({
  start: () => events.push("start"),
  error: (ctx: any) => events.push("error:" + ctx.error.message),
  end: () => events.push("end"),
});
try { ch.traceSync(() => { events.push("fn"); throw new Error("boom"); }, {}); } catch (err: any) { console.log("caught:", err.message); }
console.log("events:", events.join("|"));
