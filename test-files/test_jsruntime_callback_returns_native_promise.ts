import { awaitCallback } from "./fixtures/jsruntime_promise_surface/mod.js";

const value = await awaitCallback(() => {
  return new Promise<string>((resolve) => {
    setTimeout(resolve, 1, "inner-native");
  });
});
console.log("callback:", value);
