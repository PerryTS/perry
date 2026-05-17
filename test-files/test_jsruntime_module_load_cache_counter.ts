import { delayedValue } from "./fixtures/jsruntime_promise_surface/mod.js";

let total = "";

for (let i = 0; i < 20; i += 1) {
  total += await delayedValue("x");
}

console.log("module-cache-counter:", total.length);
