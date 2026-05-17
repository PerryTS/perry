import { awaitCallback, delayedValue } from "./fixtures/jsruntime_promise_surface/mod.js";

let completed = 0;

for (let i = 0; i < 200; i++) {
  const values = await Promise.all([
    Promise.resolve("native"),
    delayedValue("v8"),
    new Promise<string>((resolve) => {
      setTimeout(resolve, 1, "timer");
    }),
    awaitCallback(() => {
      return new Promise<string>((resolve) => {
        setTimeout(resolve, 1, "callback");
      });
    }),
  ]);
  completed += values.length;
}

console.log("liveness-stress:", completed);
