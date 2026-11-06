import { registerHooks } from "node:module";

const calls: string[] = [];
const first = registerHooks({
  resolve(specifier, context, nextResolve) {
    calls.push("first:resolve:before");
    const result = nextResolve(specifier, context);
    calls.push("first:resolve:after");
    return result;
  },
  load(url, context, nextLoad) {
    calls.push("first:load:before");
    const result = nextLoad(url, context);
    calls.push("first:load:after");
    return result;
  },
});
const second = registerHooks({
  resolve(specifier, context, nextResolve) {
    calls.push("second:resolve:before");
    const result = nextResolve(specifier, context);
    calls.push("second:resolve:after");
    return result;
  },
  load(url, context, nextLoad) {
    calls.push("second:load:before");
    const result = nextLoad(url, context);
    calls.push("second:load:after");
    return result;
  },
});
try {
  await import("./fixtures/loader-hook-first.ts");
  console.log("order:", calls.join("|"));
} finally {
  second.deregister();
  first.deregister();
}
