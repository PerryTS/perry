import * as Module from "node:module";

const calls: string[] = [];

const handle = Module.registerHooks({
  resolve(specifier: string, context: any, nextResolve: Function) {
    calls.push(`resolve:${specifier}:${typeof context.parentURL}:${typeof nextResolve}`);
    const result = nextResolve(specifier, context);
    calls.push(`resolve-result:${typeof result.url}:${result.format ?? "none"}`);
    return result;
  },
  load(url: string, context: any, nextLoad: Function) {
    calls.push(`load:${typeof url}:${typeof context.format}:${typeof nextLoad}`);
    const result = nextLoad(url, context);
    calls.push(`load-result:${result.format ?? "none"}:${"source" in result}`);
    return result;
  },
});

const firstPath =
  true ? "./fixtures/loader-hook-first.ts" : "./fixtures/loader-hook-second.ts";
const first = await import(firstPath);
console.log("first value:", first.value);
console.log("calls after first:", calls.join("|"));

handle.deregister();

const secondPath =
  false ? "./fixtures/loader-hook-first.ts" : "./fixtures/loader-hook-second.ts";
const second = await import(secondPath);
console.log("second value:", second.value);
console.log("calls after deregister:", calls.join("|"));
