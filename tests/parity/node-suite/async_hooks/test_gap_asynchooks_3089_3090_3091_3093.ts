import async_hooks, { AsyncResource, AsyncLocalStorage } from "node:async_hooks";

// #3089 — createHook option/member validation
for (const value of [undefined, null]) {
  try {
    async_hooks.createHook(value as any);
  } catch (err: any) {
    console.log("options", String(value), err.name, err.code || "no-code", err.message);
  }
}
for (const value of [null, 0, true, "x", {}, [], Symbol("s")]) {
  try {
    async_hooks.createHook({ init: value } as any);
  } catch (err: any) {
    console.log("init", String(value), err.name, err.code || "no-code", err.message);
  }
}
for (const member of ["before", "after", "destroy", "promiseResolve"]) {
  try {
    async_hooks.createHook({ [member]: 5 } as any);
  } catch (err: any) {
    console.log(member, err.name, err.code || "no-code", err.message);
  }
}
const undefMemberHook = async_hooks.createHook({ init: undefined });
console.log("undef-member-ok", typeof undefMemberHook.enable);
const primitiveOptionsHook = async_hooks.createHook(0 as any);
console.log("primitive-options-ok", typeof primitiveOptionsHook.enable);
const missingMemberHook = async_hooks.createHook({});
console.log("missing-member-ok", typeof missingMemberHook.enable);

// #3090 — AsyncResource constructor input validation
for (const value of [undefined, null, 0, true, {}, [], Symbol("s")]) {
  try {
    new AsyncResource(value as any);
  } catch (err: any) {
    console.log("type", String(value), err.name, err.code || "no-code", err.message);
  }
}
for (const value of [null, true, "x", {}, [], Symbol("s"), 1.5, NaN, Infinity]) {
  try {
    new AsyncResource("T", { triggerAsyncId: value } as any);
  } catch (err: any) {
    console.log("trigger", String(value), err.name, err.code || "no-code", err.message);
  }
}
const triggerUndefRes = new AsyncResource("T", { triggerAsyncId: undefined });
console.log("trigger-undefined-ok", typeof triggerUndefRes.asyncId());
const triggerNeg1Res = new AsyncResource("T", { triggerAsyncId: -1 });
console.log("trigger-neg1-ok", typeof triggerNeg1Res.asyncId());

// #3091 — AsyncResource callback argument validation
const ar = new AsyncResource("T");
for (const value of [undefined, null, 0, true, "x", {}, [], Symbol("s")]) {
  try {
    ar.runInAsyncScope(value as any);
  } catch (err: any) {
    console.log("run", String(value), err.name, err.code || "no-code");
  }
}
for (const value of [undefined, null, 0, true, "x", {}, [], Symbol("s")]) {
  try {
    ar.bind(value as any);
  } catch (err: any) {
    console.log("bind", String(value), err.name, err.code || "no-code", err.message);
  }
}
console.log(
  "run-args",
  ar.runInAsyncScope(
    function (this: any, ...args: any[]) {
      return JSON.stringify(args) + "|this=" + String(this);
    },
    "ctx",
    1,
    2,
    3,
  ),
);

// #3093 — AsyncLocalStorage run/exit argument forwarding
const als = new AsyncLocalStorage<string>();
console.log(
  "run ret:",
  als.run(
    "store",
    function (...args: any[]) {
      console.log("run args:", JSON.stringify(args), "store:", als.getStore());
      return "r";
    },
    "a",
    "b",
    "c",
  ),
);
console.log(
  "exit ret:",
  als.run("outer", () =>
    als.exit(
      function (...args: any[]) {
        console.log("exit args:", JSON.stringify(args), "store:", als.getStore());
        return "e";
      },
      "x",
      "y",
    ),
  ),
);
