import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-callback");
const events: string[] = [];
const context: any = { input: "ctx" };
ch.subscribe({
  start: () => events.push("start"),
  end: () => events.push("end"),
  asyncStart: (ctx: any) => events.push(`asyncStart:${ctx.result}`),
  asyncEnd: (ctx: any) => events.push(`asyncEnd:${ctx.result}`),
  error: (ctx: any) => events.push(`error:${ctx.error?.message}`),
});
ch.traceCallback((cb: Function, err: any, value: any) => {
  events.push("fn");
  cb(err, value);
}, 0, context, undefined, (err: any, value: any) => {
  console.log("callback args:", err, value);
}, null, "ok");
console.log("events:", events.join("|"));
