import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-sync-error");
const events: string[] = [];
const context: any = { input: "ctx" };
const error = new Error("sync boom");
ch.subscribe({
  start: (ctx: any) => events.push(`start:${ctx.input}`),
  end: (ctx: any) => events.push(`end:${ctx.error === error}`),
  error: (ctx: any) => events.push(`error:${ctx.error === error}`),
});
try {
  ch.traceSync(() => { throw error; }, context);
  console.log("no throw");
} catch (err) {
  console.log("caught same:", err === error);
}
console.log("events:", events.join("|"));
