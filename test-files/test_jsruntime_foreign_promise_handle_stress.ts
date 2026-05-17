import { delayedValue } from "./fixtures/jsruntime_promise_surface/mod.js";

let total = 0;

for (let batch = 0; batch < 20; batch++) {
  const promises: Promise<string>[] = [];
  for (let i = 0; i < 50; i++) {
    promises.push(delayedValue("x"));
  }
  const values = await Promise.all(promises);
  total += values.length;
}

const reused = delayedValue("same");
const first = await reused;
const second = await reused;
const both = await Promise.all([reused, reused]);

console.log("foreign-stress:", total, first, second, both.join("|"));
