import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-callback-error-first-extra");
const events: string[] = [];
ch.subscribe({
  start: () => events.push("start"),
  asyncStart: (ctx: any) => events.push("asyncStart:" + (ctx.error?.message || ctx.result)),
  asyncEnd: (ctx: any) => events.push("asyncEnd:" + (ctx.error?.message || ctx.result)),
  error: (ctx: any) => events.push("error:" + ctx.error.message),
});
ch.traceCallback((cb: Function) => cb(new Error("cbfail")), 0, {}, undefined, (err: any) => console.log("cb err:", err.message));
console.log("events:", events.join("|"));
