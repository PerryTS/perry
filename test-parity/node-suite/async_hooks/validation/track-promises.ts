import { createHook } from "node:async_hooks";

function describe(value: unknown) {
  if (value === null) return "null";
  if (typeof value === "symbol") return "symbol";
  if (typeof value === "function") return "function";
  if (Number.isNaN(value)) return "NaN";
  return String(value);
}
function probe(label: string, callback: () => unknown) {
  try {
    callback();
    console.log(label, "no-throw");
  } catch (error) {
    const caught = error as { name?: string; code?: string };
    console.log(label, caught.name, caught.code);
  }
}

for (const value of [0, null, 1, NaN, Symbol("x"), () => {}, "test"]) {
  probe(`trackPromises ${describe(value)}`, () =>
    createHook({ init() {}, trackPromises: value as any }),
  );
}
probe("trackPromises false promiseResolve", () =>
  createHook({ trackPromises: false, promiseResolve() {} }),
);
console.log(
  "trackPromises booleans:",
  typeof createHook({ init() {}, trackPromises: true }),
  typeof createHook({ init() {}, trackPromises: false }),
);
