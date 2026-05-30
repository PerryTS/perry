// Issue #3067 — `node:timers/promises` helpers reject a non-number `delay`
// and a non-object `options` with `TypeError [ERR_INVALID_ARG_TYPE]`, matching
// Node. A missing argument is allowed (delay defaults, options treated as the
// empty object); a non-null, non-array object is a valid options bag. The
// `NaN`-delay warn/coerce path is excluded here — it is tracked by #2966.
import * as timers from "node:timers/promises";

async function probe(label: string, fn: () => Promise<unknown>) {
  try {
    const result = await fn();
    console.log(label, "OK", result);
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

await probe("setTimeout string delay", () => timers.setTimeout("x" as any, "v"));
await probe("setTimeout null delay", () => timers.setTimeout(null as any, "v"));
await probe("setTimeout object delay", () => timers.setTimeout({} as any, "v"));
await probe("setTimeout no delay", () => timers.setTimeout());
await probe("setTimeout undefined delay", () => timers.setTimeout(undefined, "v"));
await probe("setTimeout options primitive", () => timers.setTimeout(0, "v", 1 as any));
await probe("setTimeout options null", () => timers.setTimeout(0, "v", null as any));
await probe("setTimeout options array", () => timers.setTimeout(0, "v", [] as any));
await probe("setTimeout options object", () => timers.setTimeout(0, "v", {}));
await probe("setTimeout options undefined", () => timers.setTimeout(0, "v", undefined));
await probe("setImmediate options primitive", () => timers.setImmediate("v", 1 as any));
await probe("setImmediate options null", () => timers.setImmediate("v", null as any));
await probe("setImmediate no options", () => timers.setImmediate("v"));
await probe("scheduler.wait string delay", () => timers.scheduler.wait("x" as any));
await probe("scheduler.wait number", () => timers.scheduler.wait(0));
