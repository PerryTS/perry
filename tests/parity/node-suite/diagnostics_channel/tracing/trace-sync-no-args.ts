import { tracingChannel } from "node:diagnostics_channel";
const trace = tracingChannel("dc-trace-no-args");
const events: string[] = [];
trace.subscribe({ start: () => events.push("start"), end: (ctx: any) => events.push("end:" + ctx.result) });
const result = trace.traceSync(() => "ok", {});
console.log("result:", result);
console.log("events:", events.join(","));
