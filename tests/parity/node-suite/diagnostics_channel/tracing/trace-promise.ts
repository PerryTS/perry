import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-promise");
const events: string[] = [];
const context: any = { input: "ctx" };
const expected = { value: "result" };
ch.subscribe({
  start: (ctx: any) => events.push(`start:${ctx.input}`),
  end: (ctx: any) => events.push(`end:${ctx.result === expected}`),
  asyncStart: (ctx: any) => events.push(`asyncStart:${ctx.result === expected}`),
  asyncEnd: (ctx: any) => events.push(`asyncEnd:${ctx.result === expected}`),
  error: (ctx: any) => events.push(`error:${ctx.error}`),
});
const value = await ch.tracePromise(() => Promise.resolve(expected), context);
console.log("value identity:", value === expected);
console.log("events:", events.join("|"));
