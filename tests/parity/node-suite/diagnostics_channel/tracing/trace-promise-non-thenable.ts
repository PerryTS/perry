import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-promise-non-thenable");
const events: string[] = [];
ch.subscribe({
  start: () => events.push("start"),
  end: () => events.push("end"),
  asyncStart: () => events.push("asyncStart"),
  asyncEnd: () => events.push("asyncEnd"),
});
const value = await ch.tracePromise(() => "plain-value", {});
console.log("value:", value);
console.log("events:", events.join("|"));
