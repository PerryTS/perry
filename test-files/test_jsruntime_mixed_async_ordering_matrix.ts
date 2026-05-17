import { awaitCallback, delayedValue } from "./fixtures/jsruntime_promise_surface/mod.js";

const events: string[] = [];

events.push("sync");
Promise.resolve().then(() => events.push("native-micro"));

await Promise.resolve();
events.push("after-native-await");

events.push("before-v8");
const v8Value = await delayedValue("one");
events.push("v8:" + v8Value);

events.push("before-timer");
const timerValue = await new Promise<string>((resolve) => {
  setTimeout(resolve, 1, "done");
});
events.push("timer:" + timerValue);

const callbackValue = await awaitCallback(() => {
  return new Promise<string>((resolve) => {
    setTimeout(resolve, 1, "inner");
  });
});
events.push("callback:" + callbackValue);

const allValues = await Promise.all([
  Promise.resolve("native"),
  delayedValue("v8"),
  new Promise<string>((resolve) => {
    setTimeout(resolve, 1, "timer");
  }),
]);
events.push("all:" + allValues.join("|"));

console.log("ordering:", events.join(">"));
