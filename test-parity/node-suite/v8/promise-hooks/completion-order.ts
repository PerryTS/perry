import { promiseHooks } from "node:v8";

const events: string[] = [];
let child: Promise<any>;
const stops = [
  promiseHooks.onBefore((promise) => {
    if (promise === child) events.push("before");
  }),
  promiseHooks.onAfter((promise) => {
    if (promise === child) events.push("after");
  }),
  promiseHooks.onSettled((promise) => {
    if (promise === child) events.push("settled");
  }),
];

child = Promise.resolve().then(() => events.push("callback"));
child.then(() => {
  for (const stop of stops) stop();
  console.log("events:", events.join(","));
});
