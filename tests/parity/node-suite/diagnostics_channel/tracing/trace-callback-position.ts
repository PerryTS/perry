import { tracingChannel } from "node:diagnostics_channel";

const active = tracingChannel("dc-trace-callback-position-active");
const activeEvents: string[] = [];
active.subscribe({
  start: (ctx: any) => activeEvents.push(`start:${ctx.label}`),
  end: (ctx: any) => activeEvents.push(`end:${ctx.result}`),
  asyncStart: (ctx: any) => activeEvents.push(`asyncStart:${ctx.result}`),
  asyncEnd: (ctx: any) => activeEvents.push(`asyncEnd:${ctx.result}`),
  error: (ctx: any) => activeEvents.push(`error:${ctx.error?.message}`),
});

function target(this: any, a: string, cb: Function, b: string, c: string) {
  activeEvents.push(`target:${[a, typeof cb, b, c, this?.tag].join(",")}`);
  cb(null, "value");
  return "target-ret";
}

const activeRet = active.traceCallback(
  target,
  1,
  { label: "ctx" },
  { tag: "this" },
  "A",
  (err: any, value: string) => {
    activeEvents.push(`callback:${err}:${value}`);
  },
  "B",
  "C",
);
console.log("active ret:", activeRet);
console.log("active events:", activeEvents.join("|"));

const inactive = tracingChannel("dc-trace-callback-position-inactive");
const inactiveEvents: string[] = [];
const inactiveRet = inactive.traceCallback(
  function (a: string, b: string, cb: Function) {
    inactiveEvents.push(`inactive target:${[a, b, typeof cb].join(",")}`);
    cb(null, "neg");
    return "inactive-ret";
  },
  -1,
  {},
  undefined,
  "X",
  "Y",
  (err: any, value: string) => {
    inactiveEvents.push(`inactive callback:${err}:${value}`);
  },
);
console.log("inactive ret:", inactiveRet);
console.log("inactive events:", inactiveEvents.join("|"));
