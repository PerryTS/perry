import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-promise-error-ordering");
const events: string[] = [];
const reason = new Error("boom");
ch.subscribe({
  start: () => events.push("start"),
  end: () => events.push("end"),
  asyncStart: () => events.push("asyncStart"),
  asyncEnd: () => events.push("asyncEnd"),
  error: (ctx: any) => events.push(`error:${ctx.error === reason}`),
});
try {
  await ch.tracePromise(() => Promise.reject(reason), {});
  console.log("did not throw");
} catch (err: any) {
  console.log("threw same:", err === reason);
}
console.log("events:", events.join("|"));
