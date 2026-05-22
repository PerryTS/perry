import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-callback-validation");
ch.subscribe({ start: () => {} });
try {
  ch.traceCallback(() => "unused", 0, {}, undefined, 1, 2, 3);
  console.log("invalid callback: no throw");
} catch (err: any) {
  console.log("invalid callback:", err?.name, err?.code || "no-code");
}
