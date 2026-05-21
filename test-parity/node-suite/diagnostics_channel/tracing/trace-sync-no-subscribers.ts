import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-sync-fast");
const thisArg = { ok: true };
const result = ch.traceSync(function (this: any, value: number) {
  console.log("fn this/value:", this === thisArg, value);
  return value + 1;
}, { context: true }, thisArg, 41);
console.log("result:", result);
console.log("hasSubscribers:", ch.hasSubscribers);
