import { delayedValue } from "./fixtures/jsruntime_promise_surface/mod.js";

const values = await Promise.all([
  Promise.resolve("native-first"),
  delayedValue("v8-second"),
]);

console.log("all:", values[0], values[1]);
