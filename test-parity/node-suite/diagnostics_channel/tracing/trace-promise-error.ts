import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-promise-error");
const events: string[] = [];
const context: any = { input: "ctx" };
const error = new Error("promise boom");
ch.subscribe({
  start: () => events.push("start"),
  end: (ctx: any) => events.push(`end:${ctx.error === error}`),
  asyncStart: (ctx: any) => events.push(`asyncStart:${ctx.error === error}`),
  asyncEnd: (ctx: any) => events.push(`asyncEnd:${ctx.error === error}`),
  error: (ctx: any) => events.push(`error:${ctx.error === error}`),
});
try {
  await ch.tracePromise(() => Promise.reject(error), context);
  console.log("no rejection");
} catch (err) {
  console.log("caught same:", err === error);
}
console.log("events:", events.join("|"));
