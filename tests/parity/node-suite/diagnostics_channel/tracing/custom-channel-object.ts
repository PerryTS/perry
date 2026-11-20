import { channel, tracingChannel } from "node:diagnostics_channel";

const start = channel("dc-custom-start");
const end = channel("dc-custom-end");
const asyncStart = channel("dc-custom-async-start");
const asyncEnd = channel("dc-custom-async-end");
const error = channel("dc-custom-error");
const trace = tracingChannel({ start, end, asyncStart, asyncEnd, error });
const events: string[] = [];
start.subscribe(() => events.push("start"));
end.subscribe(() => events.push("end"));
trace.traceSync(() => "value", {});
console.log("events:", events.join(","));
console.log("has:", trace.hasSubscribers);
