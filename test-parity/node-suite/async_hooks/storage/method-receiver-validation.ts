import { AsyncLocalStorage } from "node:async_hooks";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "ok", value === undefined ? "undefined" : String(value));
  } catch (error: any) {
    console.log(label, error.name, error.code || "no-code");
  }
}
const storage = new AsyncLocalStorage<string>();
storage.enterWith("store");
const getStore = storage.getStore;
const run = storage.run;
const enterWith = storage.enterWith;
const exit = storage.exit;
const disable = storage.disable;
probe("detached getStore", () => getStore());
probe("detached run", () => run("next", () => "result"));
probe("detached enterWith", () => enterWith("next"));
probe("detached exit", () => exit(() => "result"));
probe("detached disable", () => disable());
for (const [name, method, args] of [
  ["getStore", AsyncLocalStorage.prototype.getStore, []],
  ["run", AsyncLocalStorage.prototype.run, ["x", () => "result"]],
  ["enterWith", AsyncLocalStorage.prototype.enterWith, ["x"]],
  ["exit", AsyncLocalStorage.prototype.exit, [() => "result"]],
  ["disable", AsyncLocalStorage.prototype.disable, []],
] as const) {
  probe(`foreign ${name}`, () => (method as any).call({}, ...args));
}
storage.disable();
