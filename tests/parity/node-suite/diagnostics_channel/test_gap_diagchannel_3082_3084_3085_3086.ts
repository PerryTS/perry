import { channel, tracingChannel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

// Collect uncaught exceptions deterministically (they drain after sync code).
const uncaught: string[] = [];
process.on("uncaughtException", (err: any) => {
  uncaught.push("uncaught:" + err.name + ":" + (err.code || "no-code") + ":" + err.message);
});

// ---- #3082: runStores forwards ALL callback arguments (bound + unbound) ----
{
  const ch = channel("runstores-bound");
  const als = new AsyncLocalStorage();
  ch.bindStore(als);
  const ret = ch.runStores(
    { value: 1 },
    function (this: any, ...args: any[]) {
      console.log("3082 bound args:", JSON.stringify(args));
      console.log("3082 bound this.tag:", this && this.tag);
      console.log("3082 bound store:", JSON.stringify(als.getStore()));
      return "ret";
    },
    { tag: "ctx" },
    "a",
    "b",
    "c",
  );
  console.log("3082 bound ret:", ret);
}
{
  const ch = channel("runstores-unbound");
  const ret = ch.runStores(
    { value: 2 },
    function (this: any, ...args: any[]) {
      console.log("3082 unbound args:", JSON.stringify(args));
      console.log("3082 unbound this.tag:", this && this.tag);
      return "ret2";
    },
    { tag: "ctx2" },
    "x",
    "y",
    "z",
  );
  console.log("3082 unbound ret:", ret);
}

// ---- #3084: tracingChannel rejects symbols; channel(symbol) is still ok ----
for (const value of [Symbol("s"), Symbol.for("shared")]) {
  try {
    tracingChannel(value as any);
    console.log("3084 no throw");
  } catch (err: any) {
    console.log("3084:", err.name, err.code, err.message);
  }
}
console.log("3084 channel(symbol) ok:", typeof channel(Symbol("ok")));

// ---- #3085: non-callable bindStore transform reports TypeError, no context --
for (const transform of [null, 1]) {
  const ch = channel("bind-transform-" + String(transform));
  const als = new AsyncLocalStorage();
  ch.bindStore(als, transform as any);
  const ret = ch.runStores({ value: 9 }, () => {
    console.log("3085 store:", JSON.stringify(als.getStore()), "transform:", String(transform));
    return "ret";
  });
  console.log("3085 returned:", ret);
}
// explicit undefined transform = no-transform (store IS set)
{
  const ch = channel("bind-transform-undef");
  const als = new AsyncLocalStorage();
  ch.bindStore(als, undefined as any);
  ch.runStores({ value: 7 }, () => {
    console.log("3085 undef store:", JSON.stringify(als.getStore()));
    return "ret";
  });
}

// ---- #3086: traceCallback honors position + forwards surrounding args -------
{
  const ch = tracingChannel("tracecb-position");
  const events: string[] = [];
  ch.subscribe({
    start: (ctx: any) => events.push("start:" + JSON.stringify(ctx)),
    end: (ctx: any) => events.push("end:" + JSON.stringify(ctx)),
    asyncStart: (ctx: any) => events.push("asyncStart:" + JSON.stringify(ctx.result)),
    asyncEnd: (ctx: any) => events.push("asyncEnd:" + JSON.stringify(ctx.result)),
  });
  function target(a: any, cb: any, b: any, c: any) {
    events.push("fn args:" + [a, typeof cb, b, c].join(","));
    cb(null, "cb-value");
    return "target-ret";
  }
  const ret = ch.traceCallback(
    target as any,
    1,
    { ctx: true } as any,
    { tag: "this" } as any,
    "A",
    (err: any, value: any) => events.push("callback:" + err + ":" + value),
    "B",
    "C",
  );
  console.log("3086 ret:", ret);
  console.log(events.join("\n"));
}

// ---- flush uncaught exceptions deterministically ----
setTimeout(() => {
  for (const u of uncaught) console.log(u);
}, 0);
