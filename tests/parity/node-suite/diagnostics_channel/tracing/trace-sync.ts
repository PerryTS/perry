import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-sync");
const events: string[] = [];
const context: any = { input: "ctx" };
const thisArg = { label: "this" };
const arg = { value: "arg" };
const expected = { value: "result" };
ch.subscribe({
  start: (ctx: any) => events.push(`start:${ctx.input}:${ctx.result}`),
  end: (ctx: any) => events.push(`end:${ctx.input}:${ctx.result === expected}`),
  asyncStart: () => events.push("asyncStart"),
  asyncEnd: () => events.push("asyncEnd"),
  error: () => events.push("error"),
});
const result = ch.traceSync(function (this: any, received: any) {
  events.push(`fn:${this === thisArg}:${received === arg}`);
  return expected;
}, context, thisArg, arg);
console.log("result identity:", result === expected);
console.log("events:", events.join("|"));
ch.unsubscribe({});
