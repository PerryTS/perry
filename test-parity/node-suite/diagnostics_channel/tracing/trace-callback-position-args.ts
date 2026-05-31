import { tracingChannel } from "node:diagnostics_channel";

const active = tracingChannel("dc-trace-position-active");
const events: string[] = [];
active.subscribe({
  start: (ctx: any) => events.push(`start:${JSON.stringify(ctx)}`),
  end: (ctx: any) => events.push(`end:${JSON.stringify(ctx)}`),
  asyncStart: (ctx: any) => events.push(`asyncStart:${JSON.stringify(ctx.result)}`),
  asyncEnd: (ctx: any) => events.push(`asyncEnd:${JSON.stringify(ctx.result)}`),
  error: (ctx: any) => events.push(`error:${JSON.stringify(ctx.error?.message)}`),
});

function target(this: any, a: string, cb: Function, b: string, c: string): string {
  events.push(`fn:${[a, typeof cb, b, c, this?.tag].join(",")}`);
  cb(null, "cb-value");
  return "target-ret";
}

const ret = active.traceCallback(target, 1, { ctx: true }, { tag: "this" },
  "A", (err: any, value: any) => events.push(`callback:${err}:${value}`), "B", "C");
console.log("active ret:", ret);
console.log("active events:", events.join("|"));

const inactive = tracingChannel("dc-trace-position-inactive");
function inactiveTarget(this: any, a: string, cb: Function, b: string): string {
  console.log("inactive fn:", [a, typeof cb, b, this?.tag].join(","));
  cb("ierr", "ival");
  return "inactive-ret";
}

const inactiveRet = inactive.traceCallback(inactiveTarget, 1, { ctx: false },
  { tag: "inactive-this" }, "IA",
  (err: any, value: any) => console.log("inactive callback:", err, value), "IB");
console.log("inactive ret:", inactiveRet);
