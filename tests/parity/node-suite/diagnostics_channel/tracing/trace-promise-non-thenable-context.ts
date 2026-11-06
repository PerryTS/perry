import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-promise-non-thenable-context");
const context: any = {};
const events: string[] = [];

ch.subscribe({
  start: (ctx: any) =>
    events.push(`start:${Object.prototype.hasOwnProperty.call(ctx, "result")}`),
  end: (ctx: any) => events.push(`end:${ctx.result}`),
  asyncStart: () => events.push("asyncStart"),
  asyncEnd: () => events.push("asyncEnd"),
});

const value = ch.tracePromise(() => "plain-value", context);
console.log("return:", value, typeof (value as any)?.then);
console.log("events:", events.join("|"));
console.log("context result:", context.result);
